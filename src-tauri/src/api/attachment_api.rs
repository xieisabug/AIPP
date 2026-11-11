use crate::db::conversation_db::{AttachmentType, Repository};
use anyhow::{anyhow, Result};
#[cfg(target_os = "android")]
use base64::{engine::general_purpose::STANDARD, Engine};
use mime_guess::from_path;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tauri_plugin_opener::OpenerExt;

use crate::{
    db::conversation_db::{ConversationDatabase, MessageAttachment},
    errors::AppError,
};
use tracing::{debug, info};

#[derive(Serialize)]
pub struct AttachmentResult {
    attachment_id: i64,
}

#[tauri::command]
pub async fn add_attachment(
    app_handle: tauri::AppHandle,
    file_url: Option<String>,
    file_content: Option<String>,
    file_name: Option<String>,
    attachment_type: Option<i64>,
) -> Result<AttachmentResult, AppError> {
    info!(?file_url, ?file_name, "add_attachment called");
    // 如果有 URL，使用 add_attachment_by_url
    if let Some(url) = file_url {
        #[cfg(mobile)]
        {
            crate::mobile_files::require_attachment_path(&app_handle, Path::new(&url)).map_err(AppError::Anyhow)?;
            let result = add_attachment_by_url(app_handle, url.clone()).await;
            if result.is_err() {
                let _ = std::fs::remove_file(&url);
                let _ = std::fs::remove_file(format!("{url}.metadata.json"));
                if let Some(parent) = Path::new(&url).parent() { let _ = std::fs::remove_dir(parent); }
            }
            return result;
        }
        return add_attachment_by_url(app_handle, url).await;
    }

    // 如果有 content 和其他必要参数，使用 add_attachment_content
    if let (Some(content), Some(name), Some(att_type)) = (file_content, file_name, attachment_type)
    {
        return add_attachment_content(app_handle, content, name, att_type).await;
    }

    // 如果都没有提供有效参数，返回错误
    Err(AppError::Anyhow(
        "无效的参数：需要提供文件URL或者（文件内容、文件名、附件类型）".to_string(),
    ))
}

pub async fn add_attachment_by_url(
    app_handle: tauri::AppHandle,
    file_url: String,
) -> Result<AttachmentResult, AppError> {
    tauri::async_runtime::spawn_blocking(move || add_attachment_by_path(app_handle, file_url))
        .await.map_err(|e| AppError::Anyhow(format!("Attachment import worker: {e}")))?
}

fn add_attachment_by_path(
    app_handle: tauri::AppHandle,
    file_url: String,
) -> Result<AttachmentResult, AppError> {
    // 1. 解析文件路径
    let file_path = Path::new(&file_url).to_path_buf();
    #[cfg(mobile)]
    crate::mobile_files::require_attachment_path(&app_handle, &file_path)
        .map_err(AppError::Anyhow)?;

    // 2. 检查文件是否存在
    if !file_path.exists() {
        return Err(AppError::Anyhow(anyhow!("找不到对应的文件").to_string()));
    }

    // 3. 解析文件类型
    let file_type = from_path(&file_path).first_or_octet_stream().to_string();
    let mut file_type_classify = String::new();

    debug!(file_type, "detected file type");

    if file_type.starts_with("text/") || file_type == "application/json" {
        file_type_classify = "text".to_string();
    } else if file_type.starts_with("image/") {
        file_type_classify = "image".to_string();
    }
    debug!(file_type_classify, "file type classify");

    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    // 4. 使用不同类型的文件读取方式来进行读取
    let reader = match file_type_classify.as_str() {
        "image" => {
            // 入库前规整一次（缩放/压缩/转成模型支持的格式），不保留原图
            let bytes = std::fs::read(&file_path)?;
            prepare_image_for_prompt(&bytes, &file_url)?
        }
        "text" => {
            // 读取文本文件
            let mut file = File::open(file_path)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            content
        }
        _ => return Err(AppError::Anyhow(anyhow!("Unsupported file type").to_string())),
    };

    let mut hasher = Sha256::new();
    hasher.update(reader.as_bytes());
    let hash_str = hex::encode(hasher.finalize());

    debug!(hash = %hash_str, "computed file hash");

    // 去数据库根据sha256的数据查看是否有相同的attachment
    let option_attachment =
        db.attachment_repo().unwrap().read_by_attachment_hash(hash_str.as_str())?;
    match option_attachment {
        Some(attachment) => {
            info!(attachment_id = attachment.id, "attachment already exists with same hash");
            #[cfg(mobile)]
            if attachment.attachment_url.as_deref() != Some(file_url.as_str()) {
                // The existing row owns its original file; discard only this new picker copy.
                let _ = std::fs::remove_file(&file_url);
                let _ = std::fs::remove_file(format!("{file_url}.metadata.json"));
                if let Some(parent) = Path::new(&file_url).parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            return Ok(AttachmentResult { attachment_id: attachment.id });
        }
        None => {
            // 5. 保存到数据库
            // todo: 添加数据库配置和 CRUD 操作
            let attachment_id = match file_type_classify.as_str() {
                "image" => {
                    // 使用 BufReader 读取图片文件
                    let message_attachment =
                        db.attachment_repo().unwrap().create(&MessageAttachment {
                            id: 0,
                            message_id: -1,
                            attachment_type: AttachmentType::Image,
                            attachment_url: Some(file_url),
                            attachment_content: Some(reader),
                            attachment_hash: Some(hash_str),
                            use_vector: false,
                            token_count: Some(0),
                        })?;
                    message_attachment.id
                }
                "text" => {
                    // 使用 BufReader 读取图片文件
                    let message_attachment =
                        db.attachment_repo().unwrap().create(&MessageAttachment {
                            id: 0,
                            message_id: -1,
                            attachment_type: AttachmentType::Text,
                            attachment_url: Some(file_url),
                            attachment_content: Some(reader),
                            attachment_hash: Some(hash_str),
                            use_vector: false,
                            token_count: Some(0),
                        })?;
                    message_attachment.id
                }
                _ => return Err(AppError::Anyhow(anyhow!("Unsupported file type").to_string())),
            };

            // 6. 返回到前端 attachment_id，等待之后的 message 创建和更新
            Ok(AttachmentResult { attachment_id })
        }
    }
}

pub async fn add_attachment_content(
    app_handle: tauri::AppHandle,
    file_content: String,
    file_name: String,
    attachment_type: i64,
) -> Result<AttachmentResult, AppError> {
    info!(file_name, "add_attachment_content called");
    #[cfg(mobile)]
    if file_content.len() > 28 * 1024 * 1024 {
        return Err(AppError::Anyhow("MOBILE_FILE attachment-content: exceeds 20 MiB binary/28 MiB encoded limit".into()));
    }
    // 粘贴/拖拽的图片同样在入库前规整（哈希按规整后的内容计算，重复粘贴仍能去重）
    let file_content = if attachment_type == AttachmentType::Image as i64 && file_content.starts_with("data:") {
        let name = file_name.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let image = crate::utils::prompt_image::prepare_data_url(&file_content)
                .map_err(|e| AppError::Anyhow(format!("图片处理失败: {e}")))?;
            log_prepared_image(&name, &image);
            Ok::<_, AppError>(image.data_url())
        })
        .await
        .map_err(|e| AppError::Anyhow(format!("Image prepare worker: {e}")))??
    } else {
        file_content
    };
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;

    let mut hasher = Sha256::new();
    hasher.update(file_content.clone());
    let hash_str = hex::encode(hasher.finalize());

    debug!(hash = %hash_str, "computed content hash");

    // 去数据库根据sha256的数据查看是否有相同的attachment
    let option_attachment =
        db.attachment_repo().unwrap().read_by_attachment_hash(hash_str.as_str())?;

    match option_attachment {
        Some(attachment) => {
            info!(
                attachment_id = attachment.id,
                "attachment_content already exists with same hash"
            );
            return Ok(AttachmentResult { attachment_id: attachment.id });
        }
        None => {
            let message_attachment = db.attachment_repo().unwrap().create(&MessageAttachment {
                id: 0,
                message_id: -1,
                attachment_type: AttachmentType::try_from(attachment_type)
                    .map_err(|_| AppError::Anyhow("Invalid attachment type".into()))?,
                attachment_url: Some(file_name),
                attachment_content: Some(file_content),
                attachment_hash: Some(hash_str),
                use_vector: false,
                token_count: Some(0),
            });
            let attachment_id = match message_attachment {
                Ok(t) => t.id,
                Err(e) => return Err(AppError::from(e)),
            };
            Ok(AttachmentResult { attachment_id })
        }
    }
}

#[tauri::command]
pub async fn open_attachment_with_default_app(
    id: i64,
    app_handle: tauri::AppHandle,
) -> Result<(), AppError> {
    let db = ConversationDatabase::new(&app_handle).map_err(AppError::from)?;
    let attachment = db.attachment_repo().unwrap().read(id)?
        .ok_or_else(|| AppError::Anyhow(format!("Attachment {id} not found")))?;

    #[cfg(target_os = "android")]
    {
        let content = attachment.attachment_content.as_deref()
            .ok_or_else(|| AppError::Anyhow(format!("Attachment {id}: local content missing; restore attachment before opening")))?;
        let (data, mime, name) = if content.starts_with("data:image/") {
            let (header, data) = content.split_once(',').ok_or_else(|| AppError::Anyhow("Invalid image data URI".into()))?;
            let mime = header.trim_start_matches("data:").trim_end_matches(";base64");
            let ext = match mime { "image/jpeg" => "jpg", "image/png" => "png", "image/gif" => "gif", "image/webp" => "webp", _ => return Err(AppError::Anyhow("Unsupported image MIME".into())) };
            (data.to_string(), mime.to_string(), format!("attachment-{id}.{ext}"))
        } else {
            (STANDARD.encode(content.as_bytes()), "text/plain".into(), format!("attachment-{id}.txt"))
        };
        crate::mobile_files::mobile_file_action(app_handle, "open".into(), serde_json::json!({"name": name, "mime": mime, "data": data})).await.map_err(AppError::Anyhow)?;
        return Ok(());
    }

    let file_path =
        Path::new("attachments").join(attachment.attachment_url.as_ref().ok_or_else(|| AppError::Anyhow(format!("Attachment {id}: path missing")))?);
    let file_path = file_path.to_str().unwrap();
    debug!(file_path, "opening attachment with default app");

    let opener = app_handle.opener();
    opener.open_path(file_path, None::<&str>)?;
    Ok(())
}

/// 文件选择入口：规整图片并返回待入库的 data URL；无法识别或压不下来时直接报错给用户。
fn prepare_image_for_prompt(bytes: &[u8], file_name: &str) -> Result<String, AppError> {
    let image = crate::utils::prompt_image::prepare(bytes)
        .map_err(|e| AppError::Anyhow(format!("图片处理失败: {e}")))?;
    log_prepared_image(file_name, &image);
    Ok(image.data_url())
}

fn log_prepared_image(file_name: &str, image: &crate::utils::prompt_image::PromptImage) {
    if image.reencoded {
        info!(
            file_name,
            source = %format!("{}x{}", image.source_width, image.source_height),
            output = %format!("{}x{} {} {}B", image.width, image.height, image.mime, image.base64.len()),
            "image attachment normalized for model input"
        );
    }
}
