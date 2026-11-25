use super::types::{Access, Device};
use anyhow::Context;
use tauri::Manager;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn open(app: &tauri::AppHandle) -> anyhow::Result<Connection> {
    let dir = app.path().app_local_data_dir()?.join("host-bridge");
    std::fs::create_dir_all(&dir).context("创建设备授权目录")?;
    let conn = Connection::open(dir.join("devices.db")).context("打开本机设备授权库")?;
    init(&conn)?;
    Ok(conn)
}

pub(super) fn init(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS host_bridge_peer(
        id TEXT PRIMARY KEY, name TEXT NOT NULL, token_hash BLOB NOT NULL UNIQUE, access TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS host_bridge_send(
        device TEXT NOT NULL, request TEXT NOT NULL, turn TEXT NOT NULL UNIQUE,
        fingerprint BLOB NOT NULL, process TEXT NOT NULL, conversation INTEGER, message INTEGER,
        state TEXT NOT NULL, error TEXT, PRIMARY KEY(device,request)
    );")?;
    Ok(())
}

pub fn add(conn: &Connection, device: &Device, token: &str) -> anyhow::Result<()> {
    conn.execute("INSERT INTO host_bridge_peer(id,name,token_hash,access) VALUES(?1,?2,?3,?4)",
        params![device.id.to_string(), device.name, Sha256::digest(token.as_bytes()).to_vec(), serde_json::to_string(&device.access)?])?;
    Ok(())
}

pub fn devices(conn: &Connection) -> anyhow::Result<Vec<Device>> {
    let mut stmt = conn.prepare("SELECT id,name,access FROM host_bridge_peer ORDER BY rowid")?;
    let mut rows = stmt.query([])?;
    let mut devices = Vec::new();
    while let Some(row) = rows.next()? {
        devices.push(Device { id: row.get::<_, String>(0)?.parse()?, name: row.get(1)?,
            access: serde_json::from_str(&row.get::<_,String>(2)?)? });
    }
    Ok(devices)
}

pub fn access(conn: &Connection, token: &str) -> anyhow::Result<Option<Access>> {
    let grant: Option<String> = conn.query_row("SELECT access FROM host_bridge_peer WHERE token_hash=?1",
        [Sha256::digest(token.as_bytes()).to_vec()], |row| row.get(0)).optional()?;
    grant.map(|raw| serde_json::from_str(&raw).context("读取设备授权")).transpose()
}

pub fn paired_device(conn: &Connection, token: &str) -> anyhow::Result<Option<(String,Access)>> {
    let row: Option<(String,String)> = conn.query_row(
        "SELECT id,access FROM host_bridge_peer WHERE token_hash=?1",
        [Sha256::digest(token.as_bytes()).to_vec()], |r| Ok((r.get(0)?,r.get(1)?))
    ).optional()?;
    row.map(|(id,raw)| Ok((id,serde_json::from_str(&raw)?))).transpose()
}

pub fn revoke(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    conn.execute("DELETE FROM host_bridge_peer WHERE id=?1", [id.to_string()])?;
    Ok(())
}
