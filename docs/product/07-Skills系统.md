# Skills 系统

Skills 系统提供了技能模板功能，用户可以通过技能文件来定义和管理技能。Skills 基于文件系统扫描，支持官方技能库安装。

---

## Skills 扫描与管理

### Skills 文件系统扫描
- `skill_api.rs` Skills 管理 API
- Skills 从文件系统扫描，不存储在数据库
- `scan_skills` - 扫描技能目录
- Skills 包含名称、描述、提示词模板等
- Skills 可以包含参数占位符
- 参数化提示词模板

### Skills 源管理
- `get_skill_sources` - 获取技能源
- `get_skill_content` - 获取技能内容
- `get_skill` - 获取技能详情
- `skill_exists` - 检查技能是否存在

### Skills 目录操作
- `open_skills_folder` - 打开技能目录
- `open_skill_parent_folder` - 打开技能父目录
- `get_skills_directory` - 获取技能目录
- `delete_skill` - 删除技能

---

## 官方技能库

### 官方技能获取与安装
- `fetch_official_skills` - 获取官方技能
- `install_official_skill` - 安装官方技能
- `open_source_url` - 打开源 URL
- 从官方技能库安装预定义技能

---

## Skills 与 MCP 联动验证

### Skills 与 MCP 工具关联
- Skills 配置中可指定依赖的 MCP 工具
- 验证 MCP 工具可用性
- Skills 依赖检查
- 确保 Skills 所需工具已启用

---
相关源码:
- `src-tauri/src/api/skill_api.rs` - Skills API
- `src/hooks/useSkillsMcpValidation.ts` - Skills MCP 验证 Hook
