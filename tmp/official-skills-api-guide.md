# 官方推荐 Skills 接口数据添加指南

这份文档说明：如果你要给官方推荐接口新增一条数据，应该怎么写。

## 1. 一条数据代表什么

接口里的一个 item，代表的是一个 **Skills 来源**。

这个来源可以是：

- 一个 GitHub 仓库
- 一个 ZIP 链接

AIPP 会先下载这个来源，再识别里面有哪些可安装的 skills，最后让用户选择安装哪些。

补充说明：

- 如果配置了 `dirs`，就只按 `dirs` 里列出的目录处理
- 如果没有配置 `dirs`，AIPP 才会自动发现 skills
- 自动发现时，只识别包含 `SKILL.md` 的目录

---

## 2. 基本格式

接口顶层返回一个数组：

```json
[
  {
    "id": "better-auth-skills",
    "name": "Better Auth Skills",
    "description": "Better Auth 官方维护的一组认证与安全实践 skills。",
    "version": "2026-03",
    "source": {
      "type": "github",
      "repo": "better-auth/skills",
      "ref": "main"
    },
    "source_url": "https://github.com/better-auth/skills"
  }
]
```

---

## 3. 字段说明

### `id`

必填，全局唯一，建议使用 `kebab-case`。

例如：

```json
"id": "better-auth-skills"
```

---

### `name`

必填，给用户看的标题。

例如：

```json
"name": "Better Auth Skills"
```

---

### `description`

必填，用一句话说明这组 skills 是做什么的。

例如：

```json
"description": "Better Auth 官方维护的一组认证与安全实践 skills。"
```

---

### `version`

可选，只用于展示。

例如：

```json
"version": "2026-03"
```

---

### `source`

必填，表示这个来源是什么。

支持两种写法。

#### GitHub 仓库

```json
"source": {
  "type": "github",
  "repo": "owner/repo",
  "ref": "main"
}
```

说明：

- `type` 固定写 `github`
- `repo` 必须是 `owner/repo`
- `ref` 建议显式写上

#### ZIP 链接

```json
"source": {
  "type": "zip",
  "url": "https://example.com/my-skills.zip"
}
```

说明：

- `type` 固定写 `zip`
- `url` 必须是可以直接下载 ZIP 的地址

---

### `source_url`

建议填写，用于给用户展示来源地址。

- GitHub 来源一般写仓库主页
- ZIP 来源一般写发布页或 ZIP 地址本身

例如：

```json
"source_url": "https://github.com/better-auth/skills"
```

---

### `dirs`

可选。

如果你写了 `dirs`，AIPP 就只按你配置的目录来预览和安装。  
如果你不写 `dirs`，AIPP 就会自动扫描 archive，自己发现有哪些 skills。

可以直接把它理解成：

- `dirs` 有值：使用你手动指定的目录列表
- `dirs` 为空或不写：交给 AIPP 自动发现

格式如下：

```json
"dirs": [
  {
    "from": "skills/react-best-practices",
    "to": "react-best-practices"
  }
]
```

字段说明：

- `from`：archive 里的相对目录路径
- `to`：安装到 `~/.agents/skills/` 下的目标目录名

要求：

- `from` 必须是相对路径
- `to` 必须是单层目录名，不能写成 `foo/bar`
- 同一个 item 里，`to` 不能重复

---

## 4. 推荐写法

### 方案一：GitHub 仓库 + 自动发现

```json
{
  "id": "better-auth-skills",
  "name": "Better Auth Skills",
  "description": "Better Auth 官方维护的一组认证与安全实践 skills。",
  "version": "2026-03",
  "source": {
    "type": "github",
    "repo": "better-auth/skills",
    "ref": "main"
  },
  "source_url": "https://github.com/better-auth/skills"
}
```

这时 AIPP 会自动扫描仓库里的 skills。

---

### 方案二：GitHub 仓库 + 明确指定 dirs

```json
{
  "id": "vercel-selected-skills",
  "name": "Vercel Selected Skills",
  "description": "Vercel 官方仓库中挑选出的常用 skills。",
  "version": "2026-03",
  "source": {
    "type": "github",
    "repo": "vercel-labs/agent-skills",
    "ref": "main"
  },
  "source_url": "https://github.com/vercel-labs/agent-skills",
  "dirs": [
    {
      "from": "skills/react-best-practices",
      "to": "react-best-practices"
    },
    {
      "from": "skills/deploy-to-vercel",
      "to": "deploy-to-vercel"
    }
  ]
}
```

这时 AIPP 只会读取这两个目录，不会再自动扫描全部目录。

---

### 方案三：ZIP 链接

```json
{
  "id": "company-shared-skills",
  "name": "Company Shared Skills",
  "description": "团队内部共享的一组工程开发与代码评审 skills。",
  "version": "2026-03",
  "source": {
    "type": "zip",
    "url": "https://example.com/releases/company-shared-skills-2026-03.zip"
  },
  "source_url": "https://example.com/releases/company-shared-skills-2026-03.zip"
}
```

---

## 5. 仓库或 ZIP 里的结构要求

现在只认 `SKILL.md`。

也就是说，一个目录要被识别为 skill，目录里必须有：

```text
SKILL.md
```

例如：

```text
skills/
  react-best-practices/
    SKILL.md
  deploy-to-vercel/
    SKILL.md
```

也支持这种：

```text
repo/
  plugins/
    expo-app-design/
      skills/
        building-native-ui/
          SKILL.md
```

如果仓库根目录本身就是一个 skill，也可以：

```text
repo/
  SKILL.md
  references/
  assets/
```

如果目录里只有 `README.md`，或者只有其他 `.md` 文件，当前不会被识别为 skill。

---

## 6. 什么时候该写 `dirs`

建议按下面理解：

### 不写 `dirs`

适合：

- 仓库结构很清楚
- 希望 AIPP 自动发现所有可安装 skill
- 让用户自己在预览界面勾选

### 写 `dirs`

适合：

- 只想暴露仓库里的部分 skill
- 想明确控制安装目录名
- 不希望自动扫描把其他目录也识别出来

---

## 7. 添加前自检清单

提交前建议检查：

- `id` 是否唯一
- `name` 是否清楚
- `description` 是否清楚
- `source.type` 是否写对
- GitHub 类型是否正确填写了 `repo` 和 `ref`
- ZIP 类型是否是可直接下载的链接
- 如果写了 `dirs`，`from` 和 `to` 是否正确
- skill 目录里是否真的有 `SKILL.md`

---

## 8. 最短模板

### 自动发现

```json
{
  "id": "your-skill-pack-id",
  "name": "Your Skill Pack Name",
  "description": "一句话说明这组 skills 是什么。",
  "version": "2026-03",
  "source": {
    "type": "github",
    "repo": "owner/repo",
    "ref": "main"
  },
  "source_url": "https://github.com/owner/repo"
}
```

### 指定 dirs

```json
{
  "id": "your-skill-pack-id",
  "name": "Your Skill Pack Name",
  "description": "一句话说明这组 skills 是什么。",
  "version": "2026-03",
  "source": {
    "type": "github",
    "repo": "owner/repo",
    "ref": "main"
  },
  "source_url": "https://github.com/owner/repo",
  "dirs": [
    {
      "from": "skills/your-skill",
      "to": "your-skill"
    }
  ]
}
```
