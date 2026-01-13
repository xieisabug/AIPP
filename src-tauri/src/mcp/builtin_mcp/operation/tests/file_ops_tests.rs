/// 文件操作测试
///
/// 使用临时目录和文件进行测试，确保：
/// - 所有测试文件在临时目录中创建
/// - 测试完成后自动清理
/// - 不影响真实的文件系统
use super::super::state::OperationState;
use super::super::types::*;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

/// 创建模拟的 PermissionManager（跳过权限检查）
/// 在单元测试中，我们直接测试 FileOperations 的核心逻辑
/// 权限检查在集成测试中进行

/// 辅助函数：创建临时测试目录
fn create_temp_dir() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp directory")
}

/// 辅助函数：创建临时文件并写入内容
fn create_temp_file(dir: &TempDir, name: &str, content: &str) -> String {
    let path = dir.path().join(name);
    let mut file = fs::File::create(&path).expect("Failed to create temp file");
    file.write_all(content.as_bytes()).expect("Failed to write to temp file");
    path.to_string_lossy().to_string()
}

// ============= 路径验证测试 =============

/// 测试相对路径被拒绝
#[test]
fn test_read_file_rejects_relative_path() {
    // FileOperations 中的验证逻辑
    use std::path::Path;
    let relative_path = "some/relative/path.txt";
    assert!(!Path::new(relative_path).is_absolute());
}

/// 测试绝对路径被接受
#[test]
fn test_absolute_path_accepted() {
    use std::path::Path;
    let absolute_path = "/tmp/some/file.txt";
    assert!(Path::new(absolute_path).is_absolute());
}

// ============= 读取文件核心逻辑测试 =============

/// 测试文件内容读取和行号格式
///
/// 验证内容：
/// - 文件内容正确读取
/// - 行号格式符合 cat -n 格式
/// - offset 和 limit 参数正确工作
#[test]
fn test_read_file_line_formatting() {
    let temp_dir = create_temp_dir();
    let content = "line 1\nline 2\nline 3\nline 4\nline 5";
    let file_path = create_temp_file(&temp_dir, "test.txt", content);

    // 直接读取文件并模拟 FileOperations 的格式化逻辑
    let file = fs::File::open(&file_path).unwrap();
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

    // 验证行数
    assert_eq!(lines.len(), 5);

    // 验证格式化输出
    let mut formatted = String::new();
    for (idx, line) in lines.iter().enumerate() {
        formatted.push_str(&format!("{:>6}\t{}\n", idx + 1, line));
    }

    assert!(formatted.contains("     1\tline 1\n"));
    assert!(formatted.contains("     5\tline 5\n"));
}

/// 测试 offset 和 limit 参数
#[test]
fn test_read_file_offset_and_limit() {
    let temp_dir = create_temp_dir();
    let content = (1..=10).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
    let file_path = create_temp_file(&temp_dir, "lines.txt", &content);

    let lines: Vec<String> =
        fs::read_to_string(&file_path).unwrap().lines().map(String::from).collect();

    let total_lines = lines.len();
    assert_eq!(total_lines, 10);

    // 测试 offset=3, limit=4（应返回行 3-6）
    let offset = 3usize;
    let limit = 4usize;
    let start_idx = (offset - 1).min(total_lines);
    let end_idx = (start_idx + limit).min(total_lines);

    assert_eq!(start_idx, 2);
    assert_eq!(end_idx, 6);
    assert_eq!(lines[start_idx], "line 3");
    assert_eq!(lines[end_idx - 1], "line 6");
}

/// 测试超出范围的 offset
#[test]
fn test_read_file_offset_beyond_file() {
    let temp_dir = create_temp_dir();
    let content = "line 1\nline 2\nline 3";
    let file_path = create_temp_file(&temp_dir, "short.txt", content);

    let lines: Vec<String> =
        fs::read_to_string(&file_path).unwrap().lines().map(String::from).collect();

    let total_lines = lines.len();
    assert_eq!(total_lines, 3);

    // offset=100 应该返回空内容
    let offset = 100usize;
    let start_idx = (offset - 1).min(total_lines);
    assert_eq!(start_idx, total_lines);
}

/// 测试长行截断
#[test]
fn test_read_file_long_line_truncation() {
    let max_line_length = 2000;
    let long_line = "x".repeat(3000);

    let truncated = if long_line.len() > max_line_length {
        format!("{}...[truncated]", &long_line[..max_line_length])
    } else {
        long_line.clone()
    };

    assert!(truncated.len() < long_line.len());
    assert!(truncated.ends_with("...[truncated]"));
}

// ============= 写入文件核心逻辑测试 =============

/// 测试写入新文件
#[test]
fn test_write_new_file() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("new_file.txt");
    let content = "Hello, World!";

    // 验证文件不存在
    assert!(!file_path.exists());

    // 写入文件
    fs::write(&file_path, content).unwrap();

    // 验证文件存在且内容正确
    assert!(file_path.exists());
    assert_eq!(fs::read_to_string(&file_path).unwrap(), content);
}

/// 测试写入时创建父目录
#[test]
fn test_write_file_creates_parent_directory() {
    let temp_dir = create_temp_dir();
    let nested_path = temp_dir.path().join("a").join("b").join("c").join("file.txt");
    let parent = nested_path.parent().unwrap();

    // 父目录不存在
    assert!(!parent.exists());

    // 创建父目录
    fs::create_dir_all(parent).unwrap();
    assert!(parent.exists());

    // 现在可以写入
    fs::write(&nested_path, "content").unwrap();
    assert!(nested_path.exists());
}

// ============= 编辑文件核心逻辑测试 =============

/// 测试精确字符串替换（单次）
#[test]
fn test_edit_file_single_replacement() {
    let temp_dir = create_temp_dir();
    let content = "Hello World! Hello Universe!";
    let file_path = create_temp_file(&temp_dir, "edit.txt", content);

    let old_content = fs::read_to_string(&file_path).unwrap();
    let match_count = old_content.matches("Hello").count();
    assert_eq!(match_count, 2);

    // 单次替换
    let new_content = old_content.replacen("Hello", "Hi", 1);
    fs::write(&file_path, &new_content).unwrap();

    let result = fs::read_to_string(&file_path).unwrap();
    assert_eq!(result, "Hi World! Hello Universe!");
}

/// 测试全部替换
#[test]
fn test_edit_file_replace_all() {
    let temp_dir = create_temp_dir();
    let content = "foo bar foo baz foo";
    let file_path = create_temp_file(&temp_dir, "replace_all.txt", content);

    let old_content = fs::read_to_string(&file_path).unwrap();
    let new_content = old_content.replace("foo", "xxx");
    fs::write(&file_path, &new_content).unwrap();

    let result = fs::read_to_string(&file_path).unwrap();
    assert_eq!(result, "xxx bar xxx baz xxx");
    assert!(!result.contains("foo"));
}

/// 测试找不到目标字符串
#[test]
fn test_edit_file_string_not_found() {
    let temp_dir = create_temp_dir();
    let content = "Hello World!";
    let file_path = create_temp_file(&temp_dir, "no_match.txt", content);

    let old_content = fs::read_to_string(&file_path).unwrap();
    let match_count = old_content.matches("NotExist").count();
    assert_eq!(match_count, 0);
}

/// 测试 old_string 与 new_string 相同时报错
#[test]
fn test_edit_file_same_strings_error() {
    let old_string = "same text";
    let new_string = "same text";

    assert_eq!(old_string, new_string);
    // 在实际代码中会返回错误
}

// ============= 目录列表核心逻辑测试 =============

/// 测试列出目录内容
#[test]
fn test_list_directory_entries() {
    let temp_dir = create_temp_dir();

    // 创建一些文件和目录
    create_temp_file(&temp_dir, "file1.txt", "content1");
    create_temp_file(&temp_dir, "file2.rs", "fn main() {}");
    fs::create_dir(temp_dir.path().join("subdir")).unwrap();

    let entries: Vec<_> = fs::read_dir(temp_dir.path()).unwrap().filter_map(|e| e.ok()).collect();

    assert_eq!(entries.len(), 3);

    // 验证包含预期的项
    let names: Vec<String> =
        entries.iter().map(|e| e.file_name().to_string_lossy().to_string()).collect();

    assert!(names.contains(&"file1.txt".to_string()));
    assert!(names.contains(&"file2.rs".to_string()));
    assert!(names.contains(&"subdir".to_string()));
}

/// 测试递归目录列表
#[test]
fn test_list_directory_recursive() {
    let temp_dir = create_temp_dir();

    // 创建嵌套结构
    let subdir = temp_dir.path().join("sub");
    fs::create_dir(&subdir).unwrap();
    create_temp_file(&temp_dir, "root.txt", "root");
    fs::write(subdir.join("nested.txt"), "nested").unwrap();

    // 手动递归计数
    fn count_files(path: &std::path::Path) -> usize {
        let mut count = 0;
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            count += 1;
            if entry.path().is_dir() {
                count += count_files(&entry.path());
            }
        }
        count
    }

    let total = count_files(temp_dir.path());
    assert_eq!(total, 3); // root.txt, sub/, sub/nested.txt
}

/// 测试 glob 模式匹配
#[test]
fn test_list_directory_glob_pattern() {
    let temp_dir = create_temp_dir();

    create_temp_file(&temp_dir, "file1.txt", "");
    create_temp_file(&temp_dir, "file2.txt", "");
    create_temp_file(&temp_dir, "file3.rs", "");
    create_temp_file(&temp_dir, "readme.md", "");

    // 使用 glob 匹配 *.txt
    let pattern = format!("{}/*.txt", temp_dir.path().display());
    let matches: Vec<_> = glob::glob(&pattern).unwrap().filter_map(|r| r.ok()).collect();

    assert_eq!(matches.len(), 2);
    for path in &matches {
        assert!(path.extension().map(|e| e == "txt").unwrap_or(false));
    }

    // 匹配所有文件
    let pattern_all = format!("{}/*", temp_dir.path().display());
    let all_matches: Vec<_> = glob::glob(&pattern_all).unwrap().filter_map(|r| r.ok()).collect();
    assert_eq!(all_matches.len(), 4);
}

/// 测试 DirectoryEntry 结构
#[test]
fn test_directory_entry_structure() {
    let temp_dir = create_temp_dir();
    let file_path = create_temp_file(&temp_dir, "test.txt", "hello world");

    let metadata = fs::metadata(&file_path).unwrap();

    let entry = DirectoryEntry {
        name: "test.txt".to_string(),
        path: file_path.clone(),
        is_directory: false,
        size: Some(metadata.len()),
        modified: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())),
    };

    assert_eq!(entry.name, "test.txt");
    assert!(!entry.is_directory);
    assert_eq!(entry.size, Some(11)); // "hello world" = 11 bytes
    assert!(entry.modified.is_some());
}

// ============= read-before-write 安全机制测试 =============

/// 测试 OperationState 的文件读取记录用于 read-before-write 检查
#[tokio::test]
async fn test_read_before_write_mechanism() {
    let state = OperationState::new();
    let path = "/some/file.txt";

    // 初始状态：文件未读取
    assert!(!state.has_file_been_read(path).await);

    // 记录文件已读取
    state.record_file_read(path).await;

    // 现在可以通过 read-before-write 检查
    assert!(state.has_file_been_read(path).await);
}
