# Skills 安装整理方案（修正版）

## 1. 真实需求

这次需求的关键点不是“定义一种新的 skill 包目录规范，让别人按这个来打包”，而是：

1. **AIPP 内置一个官方推荐列表**
2. 列表里的每一项都对应一个 GitHub skills 仓库或仓库中的某个 skill
3. AIPP 为这些内置链接配一份**极简 DSL / 配置**
4. 当用户点击安装时，程序按这份配置去：
   - 下载 GitHub 内容
   - 找到 repo 里真正要安装的 skill 目录
   - 安装到 `~/.agents/skills`

所以，AIPP 能控制的是：

- 官方推荐列表
- 每个推荐项的 GitHub 来源
- 每个推荐项的安装配置

AIPP **不能控制**的是：

- 上游仓库到底长什么样
- skill 目录在 repo 里是 `skills/*`、`plugins/*/skills/*` 还是 `engineering/*`
- 上游是否有 manifest / index / symlink / docs / assets / zip 混在一起

这意味着方案重点应该是：

> **兼容各种 GitHub 仓库结构，并为“官方推荐项”内置一份非常小的安装 recipe。**

---

## 2. AIPP 当前实现给出的约束

## 2.1 当前目标安装目录

当前官方安装逻辑最终装到：

```text
~/.agents/skills
```

参考：

- `src-tauri/src/api/skill_api.rs:482-484`
- `src-tauri/src/api/skill_api.rs:561-564`

## 2.2 当前 scanner 的识别方式

当前 AIPP 对目录型 skill 的识别顺序是：

1. `SKILL.md`
2. `README.md`
3. 目录里的任意 `.md`

参考：

- `src-tauri/src/skills/scanner.rs:301-336`
- `src-tauri/src/skills/scanner.rs:443-476`

## 2.3 当前 scanner 只扫 source root 的直接子目录

`scan_directory()` 只读取 source root 的第一层 entries：

- 如果 entry 是目录，就把这个目录当作“可能的 skill 文件夹”
- 不会自动递归找更深层的 `plugins/*/skills/*`

这点非常关键。

比如：

```text
~/.agents/skills
  expo-app-design
    skills
      building-native-ui
```

当前 scanner 只会看到 `expo-app-design`，不会继续自动深入到 `expo-app-design/skills/building-native-ui`。

所以安装器必须把真正的 skill 目录**扁平化复制**到：

```text
~/.agents/skills/<skill-name>
```

而不是保留 GitHub 仓库原始嵌套层级。

参考：

- `src-tauri/src/skills/scanner.rs:254-299`

## 2.4 当前大小写匹配是大小写敏感的

当前代码直接检查：

- `SKILL.md`
- `README.md`
- 扩展名是否等于 `"md"`

所以像下面这种真实仓库结构：

```text
security/SKILL.MD
```

在当前逻辑里是有兼容风险的，因为：

- `SKILL.MD` 不等于 `SKILL.md`
- 扩展名 `MD` 也不等于 `md`

这说明安装器如果要兼容 GitHub 上真实仓库，必须补一层**大小写归一化**。

参考：

- `src-tauri/src/skills/scanner.rs:309-328`

## 2.5 当前官方 zip 安装逻辑的问题

当前 `install_official_skill` 做的是：

1. 下载 zip
2. 解压到临时目录
3. 把临时目录顶层 entry 全搬到 `~/.agents/skills`

这对 GitHub 上常见的 skills 仓库结构是不够的，因为顶层 entry 往往不是 skill，而是：

- `skills/`
- `plugins/`
- `engineering/`
- `docs/`
- `.claude-plugin/`
- `.codex/`

参考：

- `src-tauri/src/api/skill_api.rs:580-658`

---

## 3. GitHub 上真实存在的结构模式

下面是我根据公开仓库实际结构整理出的几种高频模式。

## 3.1 模式 A：根目录下是 `skills/<skill-name>`

这是最干净的一类。

### 例子

#### `vercel-labs/agent-skills`

真实路径示例：

- `skills/react-best-practices`
- `skills/deploy-to-vercel`
- `skills/composition-patterns`

注意：这个目录里还混着 `.zip` 文件，例如：

- `skills/deploy-to-vercel.zip`
- `skills/web-design-guidelines.zip`

所以“看见 `skills/` 就整个搬过去”也不够，还是要精确指到真正目录。

#### `supabase/agent-skills`

真实路径示例：

- `skills/supabase-postgres-best-practices`

目录内包含：

- `SKILL.md`
- `README.md`
- `AGENTS.md`
- `references/`

#### `remotion-dev/skills`

真实路径示例：

- `skills/remotion`

目录内包含：

- `SKILL.md`
- `rules/`

### 结论

这种仓库的推荐项配置通常只要写：

```text
repo + skills/<skill-name>
```

---

## 3.2 模式 B：插件目录里再套一层 `skills/<skill-name>`

这是第二常见模式。

### 例子

#### `expo/skills`

真实结构：

```text
plugins/
  expo-app-design/
    skills/
      building-native-ui/
      expo-api-routes/
      expo-dev-client/
```

真实路径示例：

- `plugins/expo-app-design/skills/building-native-ui`

#### `trailofbits/skills`

真实结构：

```text
plugins/
  modern-python/
    skills/
      modern-python/
```

真实路径示例：

- `plugins/modern-python/skills/modern-python`

### 结论

这类仓库不能直接把 `plugins/expo-app-design` 装进去，必须把：

```text
plugins/expo-app-design/skills/building-native-ui
```

这种真正 skill 目录抽出来，扁平安装为：

```text
~/.agents/skills/building-native-ui
```

---

## 3.3 模式 C：分类目录下直接挂 skill

### 例子

#### `alirezarezvani/claude-skills`

真实路径示例：

- `engineering/skill-tester`
- `engineering/pr-review-expert`

目录内通常包含：

- `SKILL.md`
- `README.md`
- `assets/`
- `references/`
- `scripts/`

#### `better-auth/skills`

真实路径示例：

- `better-auth/create-auth`
- `better-auth/best-practices`

### 结论

这类仓库的推荐项配置通常是：

```text
repo + category/skill-name
```

---

## 3.4 模式 D：一级目录本身就是 skill

### 例子

#### `better-auth/skills`

除了 `better-auth/create-auth` 这种分类结构外，还有：

- `security/`

而且入口文件是：

- `security/SKILL.MD`

### 结论

这说明安装器不能假设所有仓库都长成：

- `skills/<name>`
- `plugins/<name>/skills/<name>`
- `<category>/<name>`

有些仓库里，一级目录自己就是 skill。

---

## 3.5 模式 E：仓库中有 manifest / index / symlink，但它们只是辅助信息

### 例子

#### `alirezarezvani/claude-skills`

包含：

- `.claude-plugin/marketplace.json`
- `.codex/skills-index.json`
- `.codex/skills/*`（symlink mirror）

#### `expo/skills`

包含：

- `.claude-plugin/...`

#### `trailofbits/skills`

包含：

- `.claude-plugin/...`

### 结论

这些文件对 AIPP 很有价值，但更适合用来：

- 帮我们**编写官方推荐项配置**
- 帮我们理解 repo 里有哪些可安装 skill

而不是让运行时安装器强依赖它们。

尤其是 `.codex/skills/*` 这种 symlink mirror，在 Windows 和 zip 解压场景里并不稳，**不应作为安装配置里的首选路径**。  
安装配置应该优先写“真实目录”，例如：

```text
engineering/skill-tester
```

而不是：

```text
.codex/skills/skill-tester
```

---

## 4. 设计结论：官方推荐项 = GitHub 源 + 极简安装 recipe

基于上面的事实，最合适的抽象层次不是“定义新的 skill 包 DSL”，而是：

> **为 AIPP 官方推荐列表中的每一项，内置一份非常小的安装配置。**

这份配置只需要回答 3 件事：

1. 去哪下载
2. 从解压后的 repo 里拿哪几个目录
3. 这些目录安装到 `~/.agents/skills` 后叫什么名字

这就够了。

---

## 5. 极简 DSL / 配置应该长什么样

## 5.1 最推荐：直接用 JSON / TOML 配置，而不是脚本式 DSL

如果目标只是给 AIPP 内置官方推荐项，我更推荐直接用配置文件。  
本质上它已经是 DSL，只是更容易解析、更稳定。

### v1 最小字段

```json
{
  "id": "vercel-react-best-practices",
  "name": "React Best Practices",
  "source": {
    "type": "github",
    "repo": "vercel-labs/agent-skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "skills/react-best-practices",
      "to": "react-best-practices"
    }
  ]
}
```

### 这几个字段的含义

- `id`
  - AIPP 内部唯一 ID
- `name`
  - UI 展示名
- `source.repo`
  - GitHub 仓库
- `source.ref`
  - 分支 / tag / commit
- `dirs`
  - 真正要安装的 skill 目录列表
- `from`
  - 解压后 repo 内的相对路径
- `to`
  - 安装到 `~/.agents/skills` 后的目录名

这就是最小闭环。

## 5.2 如果你坚持要 DSL，建议也只保留这两个动作

文本 DSL 版本可以极简成：

```text
github vercel-labs/agent-skills#main
dir skills/react-best-practices -> react-best-practices
```

如果一个推荐项要装多个 skill：

```text
github expo/skills#main
dir plugins/expo-app-design/skills/building-native-ui -> building-native-ui
dir plugins/expo-app-design/skills/expo-api-routes -> expo-api-routes
```

我建议程序内部仍然落成结构化配置，不要真的做复杂脚本解释器。

## 5.3 v1 不要加入这些字段

为了保持最小，v1 不建议放进 DSL / 配置里的内容：

- 冲突策略
- post action
- 安装脚本
- 权限控制
- 自动发现规则
- 复杂条件判断

原因很简单：  
你这个场景下，**每个推荐项都是你预先写好的**，所以没必要让 DSL 本身承担编程语言职责。

---

## 6. 程序固定流程应该是什么

对所有官方推荐项，程序都走同一套固定流程；配置里不再描述流程，只描述目录映射。

## 6.1 固定流程

1. 用户在官方推荐列表中选中一个 skill
2. AIPP 读取该 skill 对应的内置配置
3. 根据 `repo + ref` 下载 GitHub archive
4. 解压到临时目录
5. 去掉 GitHub archive 外层 wrapper 目录
6. 依次处理 `dirs`
7. 把每个 `from` 目录复制到 `~/.agents/skills/<to>`
8. 调用 `scan_skills`
9. 清理临时目录

## 6.2 为什么这样最合适

因为这样以后不管上游 repo 是：

- `skills/*`
- `plugins/*/skills/*`
- `engineering/*`
- `better-auth/*`

对安装器来说都一样：

> **只要配置里写清楚 `from -> to`，剩下流程全固定。**

---

## 7. 安装器必须补的兼容能力

虽然 DSL 已经很小，但安装器自身必须比现在更兼容。

## 7.1 必须支持 GitHub archive 的 wrapper 目录

GitHub 源码 zip 解压后通常会多一层：

```text
repo-main/
repo-<sha>/
```

配置里的 `from` 应该理解为“相对于仓库根”的路径，而不是相对于 zip 根。

也就是说，安装器应自动剥掉最外层 wrapper。

## 7.2 必须做路径安全校验

对每个 `from`：

- 必须存在
- 必须是目录
- 必须在解压根目录之内
- 不允许 `..` 逃逸

## 7.3 必须做入口文件校验

安装前至少确认目录里有一个 AIPP 可识别的入口文档。

建议顺序：

1. 大小写不敏感匹配 `SKILL.md`
2. 大小写不敏感匹配 `README.md`
3. 任意大小写的 `.md`

这一步是为了兼容真实仓库里的：

- `SKILL.md`
- `SKILL.MD`
- `README.md`

## 7.4 最好在复制前做文件名归一化

如果目录里只有：

```text
SKILL.MD
```

建议在复制到目标目录前，统一改成：

```text
SKILL.md
```

因为当前 AIPP scanner 是大小写敏感的。  
这样可以避免安装成功但扫描不到。

## 7.5 必须复制整个 skill 目录，而不是只复制 markdown

真实 skill 目录里往往还包含：

- `references/`
- `assets/`
- `templates/`
- `scripts/`
- `rules/`

所以安装单元必须是“整个目录”。

---

## 8. 对“官方推荐列表”的推荐建模方式

我建议把“官方推荐列表”和“下载 recipe”合并在一起建模。

## 8.1 一个推荐项的最小数据结构

```json
{
  "id": "supabase-postgres-best-practices",
  "name": "Supabase Postgres Best Practices",
  "description": "Postgres performance optimization guidelines from Supabase.",
  "source": {
    "type": "github",
    "repo": "supabase/agent-skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "skills/supabase-postgres-best-practices",
      "to": "supabase-postgres-best-practices"
    }
  ]
}
```

## 8.2 一个推荐项可以装多个 skill

如果你想把一个“推荐卡片”做成 bundle，也没问题：

```json
{
  "id": "expo-app-design-bundle",
  "name": "Expo App Design Bundle",
  "source": {
    "type": "github",
    "repo": "expo/skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "plugins/expo-app-design/skills/building-native-ui",
      "to": "building-native-ui"
    },
    {
      "from": "plugins/expo-app-design/skills/expo-api-routes",
      "to": "expo-api-routes"
    }
  ]
}
```

这样“推荐列表”既可以是：

- 单 skill 推荐
- 多 skill bundle

都不用扩展 DSL 复杂度。

---

## 9. 真实仓库下的配置示例

下面这些例子就是我认为最符合你需求的“官方推荐项配置”。

## 9.1 Vercel - React Best Practices

```json
{
  "id": "vercel-react-best-practices",
  "name": "React Best Practices",
  "source": {
    "type": "github",
    "repo": "vercel-labs/agent-skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "skills/react-best-practices",
      "to": "react-best-practices"
    }
  ]
}
```

## 9.2 Supabase - Postgres Best Practices

```json
{
  "id": "supabase-postgres-best-practices",
  "name": "Supabase Postgres Best Practices",
  "source": {
    "type": "github",
    "repo": "supabase/agent-skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "skills/supabase-postgres-best-practices",
      "to": "supabase-postgres-best-practices"
    }
  ]
}
```

## 9.3 Expo - Building Native UI

```json
{
  "id": "expo-building-native-ui",
  "name": "Expo Building Native UI",
  "source": {
    "type": "github",
    "repo": "expo/skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "plugins/expo-app-design/skills/building-native-ui",
      "to": "building-native-ui"
    }
  ]
}
```

## 9.4 Trail of Bits - Modern Python

```json
{
  "id": "trailofbits-modern-python",
  "name": "Modern Python",
  "source": {
    "type": "github",
    "repo": "trailofbits/skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "plugins/modern-python/skills/modern-python",
      "to": "modern-python"
    }
  ]
}
```

## 9.5 Better Auth - Create Auth

```json
{
  "id": "better-auth-create-auth",
  "name": "Better Auth Create Auth",
  "source": {
    "type": "github",
    "repo": "better-auth/skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "better-auth/create-auth",
      "to": "create-auth"
    }
  ]
}
```

## 9.6 Better Auth - Security

```json
{
  "id": "better-auth-security",
  "name": "Better Auth Security",
  "source": {
    "type": "github",
    "repo": "better-auth/skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "security",
      "to": "security"
    }
  ]
}
```

这个例子特别有价值，因为它验证了：

- skill 不一定在 `skills/*`
- skill 不一定在 `plugins/*/skills/*`
- 入口文件大小写可能不规范

## 9.7 Alireza - Skill Tester

```json
{
  "id": "skill-tester",
  "name": "Skill Tester",
  "source": {
    "type": "github",
    "repo": "alirezarezvani/claude-skills",
    "ref": "main"
  },
  "dirs": [
    {
      "from": "engineering/skill-tester",
      "to": "skill-tester"
    }
  ]
}
```

---

## 10. manifest / index 在这个方案里的角色

上一版设计里，我把 manifest / index 放得太重了；这次修正一下：

## 10.1 它们对“作者”很有用

比如：

- `.claude-plugin/marketplace.json`
- `.codex/skills-index.json`

它们可以帮助 AIPP 维护者：

- 更快发现 repo 里有哪些 skill
- 确认分类和 source path
- 编写官方推荐项配置

## 10.2 但 v1 运行时不必强依赖它们

对 AIPP 用户来说，真正要执行安装时，最简单稳定的方式还是：

> **用你预写好的 `from -> to` 列表。**

这样运行时就不需要：

- 解析各种三方 manifest
- 处理 symlink mirror
- 理解每家 repo 自己的插件模型

---

## 11. 推荐的项目内落地方式

## 11.1 内置文件位置

建议把官方推荐列表做成本地静态配置，例如：

```text
src-tauri/resources/official_skill_recipes.json
```

或者每项一个文件：

```text
src-tauri/resources/official-skills/
  vercel-react-best-practices.json
  expo-building-native-ui.json
  better-auth-security.json
```

如果你后续想维护方便，我更推荐“一项一个文件”。

## 11.2 前端展示字段

前端卡片只需要展示：

- name
- description
- repo / source_url
- optional tags

点击安装时，把 `id` 传给后端即可。

## 11.3 后端接口建议

建议把“官方列表”改成内置配置驱动，而不是单靠远程 API 返回 `download_url`。

可以考虑：

- `list_official_skills()`：返回本地内置推荐项
- `install_official_skill(id)`：按本地 recipe 下载并安装

如果以后仍想保留远程列表，也建议远程返回的不是“裸 zip 地址”，而是同样结构的 recipe 数据。

---

## 12. 这套方案和上一版的区别

上一版我理解偏了，错误地把重点放在：

- 重新定义上游 zip 包结构
- 在 zip 内放新的 `skills.install`

但你真正要的是：

> **AIPP 官方推荐列表里的每个 GitHub skill 链接，都有一份内置的极简安装配置。**

所以修正后的核心思路是：

- **不要求上游仓库改结构**
- **不要求上游提供新的 DSL**
- **DSL / 配置由 AIPP 自己维护**
- **DSL / 配置只描述 `repo + dirs(from -> to)`**

这才是你这个场景里最稳、最简单、最可维护的方案。

---

## 13. 最终结论

如果你要做“官方推荐 skills 列表”，我建议最终方案就定成下面这样：

## 13.1 抽象层次

一个官方推荐项 = 一个 GitHub 来源 + 一份极简安装配置

## 13.2 极简配置的最小内容

只保留：

- GitHub repo
- ref
- 一组 `from -> to`

即：

```json
{
  "source": { "repo": "...", "ref": "main" },
  "dirs": [
    { "from": "...", "to": "..." }
  ]
}
```

## 13.3 安装器固定流程

固定执行：

```text
download -> extract -> strip wrapper -> validate from -> normalize names -> copy whole dir -> scan_skills -> cleanup
```

## 13.4 这套方案的优点

- 完全兼容 GitHub 上已有的各种 skill 结构
- 不要求上游改仓库
- 不需要复杂 DSL
- 不依赖运行时解析第三方 manifest
- 能明确控制“到底安装 repo 里的哪个目录”
- 能适配 AIPP 当前 scanner 的扁平 root 模型

---

## 14. 下一步如果继续实现，建议直接做这 4 件事

1. 把官方推荐列表改成内置配置驱动
2. `install_official_skill` 改为按 `id` 查 recipe 执行安装
3. 安装器补上：
   - wrapper dir 剥离
   - 路径校验
   - 大小写不敏感 markdown 入口识别
   - `SKILL.MD -> SKILL.md` 归一化
4. 安装后统一 flatten 到 `~/.agents/skills/<to>`

如果你愿意，下一步我可以继续把这份设计往下细化成：

- `official_skill_recipes.json` 的最终 schema
- Rust 结构体定义
- `install_official_skill(id)` 的伪代码 / 实现步骤
- 当前 UI 和后端需要改动的点
