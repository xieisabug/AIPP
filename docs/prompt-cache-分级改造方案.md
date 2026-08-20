# Prompt Cache 分级改造方案

## 背景

当前 AIPP 的缓存问题，不像是“完全没有缓存”，更像是：

1. provider 自己做了一部分自动前缀缓存，所以**有缓存**
2. AIPP 没有把缓存结构设计好，所以**命中率不高且不稳定**
3. 普通 Chat / Butler / tool result / 附件 / 标题生成这些请求混在一起，会进一步拉低整体观感

核心目标不是追求“所有内容都缓存”，而是尽量做到：

- **稳定的大前缀长期不变**
- **动态的新内容尽量只占很小一段尾巴**
- **大块 tool result / 附件不要反复原样重放**

---

## Level 1：低风险，高收益，先把缓存显式接起来

### 目标

先把“应该被缓存的稳定前缀”显式标记出来，让缓存行为变得可控。

### 建议

1. 给稳定的 system prompt 显式加 cache hint
   - 普通 Chat：先只给 system message 打标
   - Anthropic：优先走 **message-level cache_control**
   - OpenAI-like：再叠 request-level cache 参数 / prompt_cache_key

2. 给请求增加分类埋点
   - `main_chat`
   - `tool_continue`
   - `title_generation`
   - `summary`

3. 后续看缓存率时，优先看 `main_chat`
   - 避免被标题生成、总结等旁路请求稀释

### 预期效果

- 缓存行为更稳定
- 先区分“主聊天命中率”和“全量请求命中率”
- 不改架构，也能先拿到一波收益

---

## Level 2：减少 prompt 抖动

### 目标

减少那些本不该进入长期上下文、却持续改变前缀的动态内容。

### 建议

1. Butler 的动态 system 信息不要再写进长期历史
   - `<butler_task_result>`
   - `<butler_task_attention>`

2. 改成“本轮临时注入”，不要持久化进主会话历史
   - 可以走 `runtime_user_prompt_prefix`
   - 或做专门的 runtime injected context

3. system prompt 里不要放时变信息
   - 例如日期、当前状态、动态统计
   - 这类信息应该本轮临时给，不要长期写死在 system history

### 预期效果

- Butler 的长会话前缀更稳定
- 历史上下文不再被内部运行时事件持续污染

---

## Level 3：解决普通 Chat 的大头问题

### 目标

减少大块新内容反复进入 prompt，特别是 tool result 和附件。

### 建议

1. tool result 改成“摘要 + 引用”
   - 完整结果保存在外部状态/DB/artifact/file
   - prompt 里只放：
     - 执行了什么
     - 关键结论
     - 关键报错
     - 引用 ID

2. 附件改成“上传一次，后续引用”
   - 首轮抽取内容、做摘要、做分块
   - 后续只带：
     - attachment_id / hash / path / file_id
     - 命中的片段
     - 附件摘要

3. 老的 tool result 自动微压缩
   - 最近几条保留全文
   - 更早的只保留摘要
   - 再早的只留 placeholder + ref

### 预期效果

- 普通 Chat 的缓存率会比现在明显更高
- 越长的会话收益越明显

---

## Level 4：做成成熟产品的上下文架构

### 目标

把上下文正式分层，让“稳定骨架”和“动态尾巴”分开管理。

### 推荐结构

1. **固定骨架层**
   - system rules
   - assistant rules
   - 工具规范
   - Skills / MCP 稳定目录

2. **半稳定层**
   - 会话摘要
   - 工作状态摘要
   - 附件摘要
   - 文件索引

3. **动态尾巴层**
   - 最近几轮 user / assistant
   - 最近少量 tool result

4. **按需检索层**
   - 需要时再取全文
   - 不把完整历史每轮都重新喂进去

### 预期效果

- 才有机会逼近高缓存率产品的表现
- 普通 Chat 和 Butler 都能受益

---

## 推荐推进顺序

### 第一阶段

1. 做完 Level 1
2. 顺手做 Level 2 里的 Butler 动态 system 改造

### 第二阶段

3. 做 tool result 摘要化
4. 做附件引用化

### 第三阶段

5. 上下文分层
6. 更系统地改总结 / 检索 / 长会话重放策略

---

## 只挑 3 件最值的先做

如果只想先做收益最高的 3 件事，优先：

1. **给稳定 system prompt 显式加缓存策略**
2. **Butler 动态回流不要再写进长期历史**
3. **tool result 不再每轮原样全文回放**

---

## 一句话结论

AIPP 现在更像是“有自动缓存，但上下文结构不利于高命中”。  
真正的优化方向不是幻想“所有内容都缓存”，而是把系统改成：

> **稳定骨架可长期复用，动态内容只保留很小的尾巴，大块结果和附件尽量引用而不是反复重放。**
