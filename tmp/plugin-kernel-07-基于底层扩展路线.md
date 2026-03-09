# 基于 V2 底层的扩展路线（Theme / Markdown / 后续类型）

> 这份文档回答一个核心问题：  
> **在已经收敛好的 V2 底层（Registry + Runtime + Capability + EventBridge）上，后续怎么安全、可持续地扩展新类型。**

---

## 1. 前置共识：先扩能力，再扩类型

V2 当前稳定内核是三类：

- `assistant`
- `ui`
- `worker`

后续新增能力（如 theme、markdown、tool、export）**默认先作为“能力包”扩展**，而不是立刻增加新的一级 kind。  
只有满足“跨插件复用高 + 生命周期独立 + 权限边界清晰”三条件，才升级为独立 kind。

---

## 2. 底层扩展的统一流程（五步法）

每个新能力域都走同一条流水线：

1. **定义能力域边界**  
   例：Theme 只负责 token/样式变量，不碰会话逻辑。
2. **补 Manifest schema**  
   新增 `contributions.xxx` 声明结构（仅声明，不含可执行代码）。
3. **补 Capability API + 权限项**  
   增加最小 API，配套白名单权限。
4. **接入 Runtime 激活链路**  
   插件启用时注册贡献；停用时可 `dispose` 清理。
5. **补 Host 侧渲染与验收测试**  
   由宿主负责最终渲染/应用，不允许插件直接侵入宿主内部状态。

---

## 3. Theme 扩展路线（建议第一批）

### 3.1 目标

支持插件声明主题方案（颜色 token、代码高亮主题、可选图标集），并可在运行时切换/回滚。

### 3.2 Manifest 扩展

```json
{
  "contributions": {
    "themes": [
      {
        "id": "ocean-dark",
        "label": "Ocean Dark",
        "mode": "dark",
        "tokens": {
          "--background": "222.2 84% 4.9%",
          "--foreground": "210 40% 98%"
        },
        "codeTheme": "one-dark"
      }
    ]
  }
}
```

### 3.3 Capability 扩展（最小 API）

```ts
interface ThemeCapability {
  registerTheme(themeId: string): Disposable;
  applyTheme(themeId: string): Promise<void>;
  getActiveTheme(): Promise<string | null>;
}
```

### 3.4 权限建议

- `theme.read`
- `theme.apply`

未声明权限调用时返回错误，不做静默降级。

### 3.5 宿主落地点

1. 设置页增加“插件主题”分组
2. 应用主题时仅允许改白名单 CSS 变量
3. 切换失败回滚到上一个已生效主题

### 3.6 验收标准

- 启/停插件时主题可正确注册与清理
- 切换主题不影响聊天主流程
- 重启后可恢复上次主题选择

---

## 4. Markdown 扩展路线（建议第二批）

### 4.1 目标

支持插件扩展 markdown 渲染（代码块/指令/内联语法），同时保证渲染安全和性能稳定。

### 4.2 Manifest 扩展

```json
{
  "contributions": {
    "markdown": {
      "codeBlockLanguages": ["mermaid", "chart"],
      "directives": ["warning", "tip"]
    }
  }
}
```

### 4.3 Capability 扩展（最小 API）

```ts
interface MarkdownCapability {
  registerCodeBlockRenderer(language: string, rendererId: string): Disposable;
  registerDirective(name: string, rendererId: string): Disposable;
}
```

> 这里注册的是 **rendererId**，由宿主映射到受控渲染器，不让插件直接操纵宿主 markdown pipeline 内部对象。

### 4.4 权限建议

- `markdown.register`
- `markdown.render`

### 4.5 安全与性能约束

1. 禁止注入任意 script
2. 渲染超时/异常必须隔离，不影响消息主渲染
3. 对高成本渲染（如图表）增加懒渲染和缓存

### 4.6 验收标准

- 注册/反注册后行为可预测
- 插件异常时消息渲染仍可用
- 长消息场景无明显卡顿

---

## 5. Tool / Export / Message 等能力如何继续扩

沿用同样方法，不先加 kind，先做能力包：

1. `tool.*`：命令注册、快捷动作、工具调用审计  
2. `export.*`：导出器注册、格式能力声明、权限控制  
3. `message.*`：发送前/渲染前处理钩子（必须可关闭、可观测）

当某能力包出现以下信号，再升级为独立 kind：

- 生命周期明显独立（例如必须全局常驻）
- 需要独立管理界面和开关策略
- 权限与执行模型已稳定超过一个版本

---

## 6. 版本与兼容策略

### 6.1 版本声明

建议在 manifest 增加：

```json
{
  "compat": {
    "aipp": ">=0.0.420",
    "capabilityVersion": "2.1"
  }
}
```

### 6.2 兼容原则

1. 新 capability 只增不破
2. 旧插件继续按 `assistantType -> assistant` 映射运行
3. 不支持的 capability 明确报错，不隐式 fallback

---

## 7. 推荐实施顺序（从低风险到高价值）

1. Theme（低耦合、快见效）
2. Markdown（中等复杂，价值高）
3. Tool（涉及命令与权限治理）
4. Export（与现有导出链路对齐）
5. Message（对主链路影响最大，放最后）

---

## 8. 扩展完成定义（DoD）

每新增一个能力域，必须同时满足：

1. Manifest schema 已定稿并校验
2. Capability API 与权限白名单已落地
3. Runtime 注册/停用可清理
4. Host 渲染或执行链路有错误隔离
5. 至少一个示例插件跑通
6. 有最小测试覆盖（注册、调用、异常、卸载）

---

## 9. 一句话总结

后续的 Theme / Markdown / Tool 等扩展，都建立在同一个底座上：  
**先能力包化，后类型化；先治理，再规模化。**
