mod api;
mod config;
mod debug;
mod events;
mod interaction;
mod relay;
mod runtime;
mod types;

// ── Public re-exports ──────────────────────────────────────────────

pub use debug::{
    debug_build_feishu_interactive_payload, debug_build_feishu_markdown_card,
    debug_describe_feishu_markdown_blocks,
};
pub use types::{FeishuButlerState, FeishuDebugSendResult, FeishuRuntimeStatus};

pub(crate) use api::{
    try_deliver_acp_permission_to_feishu, try_deliver_operation_permission_to_feishu,
};
pub(crate) use config::{
    clear_feishu_secret, migrate_secure_storage_if_needed, save_feishu_secret,
};
pub(crate) use debug::resend_message_to_feishu_for_debug;
pub(crate) use interaction::try_deliver_ask_user_question_to_feishu;
pub(crate) use relay::{
    conversation_has_feishu_target, inherit_latest_feishu_target,
    maybe_schedule_butler_feishu_relay_for_aipp_turn,
};
pub(crate) use runtime::{get_runtime_status, refresh_runtime, refresh_runtime_async};

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::api::*;
    use super::debug::*;
    use super::events::*;
    use super::interaction::*;
    use super::relay::*;
    use super::types::*;

    use chrono::Utc;
    use serde_json::{json, Map, Value};

    use crate::mcp::builtin_mcp::interaction::{AskUserQuestionItem, AskUserQuestionRequestEvent};
    #[test]
    fn split_markdown_blocks_extracts_table() {
        let blocks = split_markdown_into_feishu_blocks(
            "# Title\n\n| Name | Value |\n| --- | --- |\n| A | **1** |\n| B | [2](https://example.com) |\n\nTail",
        );

        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("# Title"))
        );
        assert!(matches!(
            &blocks[1],
            FeishuCardBlock::Table(table)
                if table.headers == vec!["Name".to_string(), "Value".to_string()]
                && table.rows.len() == 2
        ));
        assert!(
            matches!(&blocks[2], FeishuCardBlock::Markdown(content) if content.contains("Tail"))
        );
    }

    #[test]
    fn split_markdown_blocks_ignores_table_inside_code_fence() {
        let blocks = split_markdown_into_feishu_blocks(
            "```markdown\n| Name | Value |\n| --- | --- |\n| A | B |\n```\n",
        );

        assert_eq!(blocks.len(), 1);
        assert!(
            matches!(&blocks[0], FeishuCardBlock::Markdown(content) if content.contains("```markdown"))
        );
    }

    #[test]
    fn build_feishu_markdown_card_renders_table_blocks_as_markdown_elements() {
        let card = build_feishu_markdown_card(
            "# Summary\n\n- item 1\n- item 2\n\n| Name | Status |\n| --- | --- |\n| A | ~~done~~ |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["tag"], "markdown");
        let content = elements[0]["content"].as_str().expect("markdown content");
        assert!(content.contains("# Summary"));
        assert!(content.contains("| Name | Status |\n| --- | --- |\n| A | ~~done~~ |"));
        assert_eq!(card["config"]["update_multi"], true);
    }

    #[test]
    fn build_feishu_markdown_card_preserves_live_outline_with_headings_and_table() {
        let card = build_feishu_markdown_card(
            "我来帮你系统性地梳理直播监管系统的全流程节点。\n\n---\n\n## 📋 直播监管系统全流程节点梳理\n\n### 第一阶段：前期准备\n| 节点 | 核心内容 |\n|------|----------|\n| **政策合规调研** | 网信办、广电总局等监管政策研究；各地区法规差异；合规红线梳理 |\n| **市场调研** | 竞品分析；市场规模；商业模式 |\n| **技术预研** | 音视频编解码、实时流处理、AI审核模型能力边界、存证技术选型 |\n\n### 第二阶段：设计与规划\n| 节点 | 核心内容 |\n|------|----------|\n| **需求分析** | 功能需求；非功能需求 |\n| **系统架构设计** | 整体技术架构、数据流设计、模块划分、扩展性设计 |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(card["config"]["update_multi"], true);
        assert_eq!(elements.len(), 1);
        assert!(elements.iter().all(|element| element["tag"] == "markdown"));
        let content = elements[0]["content"].as_str().expect("markdown");
        assert!(content.contains("## 📋 直播监管系统全流程节点梳理"));
        assert!(content.contains("| 节点 | 核心内容 |\n| --- | --- |\n| **政策合规调研** | 网信办、广电总局等监管政策研究；各地区法规差异；合规红线梳理 |\n| **市场调研** | 竞品分析；市场规模；商业模式 |\n| **技术预研** | 音视频编解码、实时流处理、AI审核模型能力边界、存证技术选型 |"));
        assert!(content.contains("| 节点 | 核心内容 |\n| --- | --- |\n| **需求分析** | 功能需求；非功能需求 |\n| **系统架构设计** | 整体技术架构、数据流设计、模块划分、扩展性设计 |"));
    }

    #[test]
    fn parse_markdown_table_handles_alignment_escaped_pipes_and_irregular_rows() {
        let table = parse_markdown_table(&[
            "| Name | Value \\| Detail | Score |".to_string(),
            "| :--- | :------------- | ----: |".to_string(),
            "| Alice | `A\\|B` | 42 |".to_string(),
            "| Bob | plain |".to_string(),
            "| Carol | too | many | columns |".to_string(),
        ])
        .expect("table should parse");

        assert_eq!(
            table.headers,
            vec!["Name".to_string(), "Value | Detail".to_string(), "Score".to_string()]
        );
        assert_eq!(
            table.rows,
            vec![
                vec!["Alice".to_string(), "`A|B`".to_string(), "42".to_string()],
                vec!["Bob".to_string(), "plain".to_string(), String::new()],
                vec!["Carol".to_string(), "too".to_string(), "many".to_string()],
            ]
        );
    }

    #[test]
    fn split_markdown_blocks_keeps_invalid_table_like_text_as_markdown() {
        let blocks = split_markdown_into_feishu_blocks(
            "Value A | Value B\nThis line is not a markdown separator\nnext line",
        );

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            FeishuCardBlock::Markdown(content)
                if content.contains("Value A | Value B")
                && content.contains("This line is not a markdown separator")
        ));
    }

    #[test]
    fn build_feishu_markdown_card_supports_multiple_tables_and_markdown_blocks() {
        let card = build_feishu_markdown_card(
            "前言\n\n| Key | Value |\n| --- | --- |\n| A | 1 |\n\n中间段落\n\n| Env | Status |\n| --- | --- |\n| Prod | **OK** |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["tag"], "markdown");
        let content = elements[0]["content"].as_str().expect("markdown content");
        assert!(content.contains("前言"));
        assert!(content.contains("| Key | Value |\n| --- | --- |\n| A | 1 |"));
        assert!(content.contains("中间段落"));
        assert!(content.contains("| Env | Status |\n| --- | --- |\n| Prod | **OK** |"));
    }

    #[test]
    fn build_feishu_markdown_card_coalesces_live_fallback_outline_into_single_markdown_element() {
        let card = build_feishu_markdown_card(
            "我来帮你系统性地梳理直播监管系统的全流程节点。你提出的方向是对的，但还需要补充一些关键环节。\n\n---\n\n## 📋 直播监管系统全流程节点梳理\n\n### 第一阶段：前期准备\n| 节点 | 核心内容 |\n|------|----------|\n| **政策合规调研** | 网信办、广电总局等监管政策研究；各地区法规差异；合规红线梳理 |\n| **市场调研** | 竞品分析（如阿里云内容安全、腾讯云天御、数美科技等）；市场规模；商业模式 |\n| **技术预研** | 音视频编解码、实时流处理、AI审核模型能力边界、存证技术选型 |\n\n### 第二阶段：设计与规划\n| 节点 | 核心内容 |\n|------|----------|\n| **需求分析** | 功能需求（你提到的采集/审核/固证/上报）；非功能需求（并发、延迟、准确率） |\n| **系统架构设计** | 整体技术架构、数据流设计、模块划分、扩展性设计 |\n| **安全与隐私设计** | 数据分级、脱敏策略、访问控制、审计日志设计 |\n\n### 第三阶段：核心能力建设\n| 节点 | 核心内容 |\n|------|----------|\n| **采集接入层** | 多平台直播流拉取（RTMP/FLV/HLS）、弹幕/评论采集、主播元数据获取 |\n| **预处理与转码** | 音视频解码、分片/切片、关键帧提取、音频转文本（ASR）、多分辨率适配 |\n| **AI审核引擎** | 图像识别（违规画面、OCR文字）、音频审核（ASR+敏感词）、多模态融合分析 |\n| **人工审核平台** | 审核工作台、多级复核机制、标注回流、审核员权限管理 |\n\n### 第四阶段：证据与合规\n| 节点 | 核心内容 |\n|------|----------|\n| **固证存证系统** | 证据链生成（截图/录屏/时间戳）、区块链/公证处存证、证据包封装 |\n| **预警与上报** | 分级预警机制、监管上报接口、处置决策（警告/断流/封号） |\n\n### 第五阶段：支撑体系\n| 节点 | 核心内容 |\n|------|----------|\n| **数据存储与检索** | 海量音视频存储、冷热分层、快速检索、生命周期管理 |\n| **运营与模型迭代** | 审核规则配置、模型效果监控、误报/漏报分析、模型热更新 |\n| **系统运维** | 高可用保障、监控告警、容量规划、容灾备份 |\n| **安全防护** | 防攻击（DDoS/CC）、防爬取、数据防泄漏、系统加固 |\n\n### 第六阶段：交付与持续运营\n| 节点 | 核心内容 |\n|------|----------|\n| **测试验收** | 功能测试、性能压测、安全测试、准确率验证 |\n| **法务合规审查** | 隐私协议、用户授权、责任边界、法律意见书 |",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        let content = elements[0]["content"].as_str().expect("markdown content");
        assert!(content.contains("## 📋 直播监管系统全流程节点梳理"));
        assert!(content.contains("### 第六阶段：交付与持续运营"));
    }

    #[test]
    fn build_feishu_markdown_card_coalesces_tog_outline_with_many_tables_into_single_markdown_element(
    ) {
        let card = build_feishu_markdown_card(
            "针对 **ToG（政府端）** 产品，我重新梳理。前期调研的核心是**搞清楚政府监管需要什么数据、能从哪里拿到数据、数据质量和成本如何**。\n\n以下是调整后的 **ToG 前期调研全流程难点分析**：\n\n---\n\n## 一、政策与考核指标调研（政府视角）\n\n| 难点 | 具体挑战 |\n|------|----------|\n| **多级考核体系** | 中央网信办\"清朗\"专项行动 vs 省级网信办考核 vs 市级属地管理要求，三级指标可能冲突（中央要\"宏观态势\"，基层要\"具体线索\"） |\n| **政绩导向模糊** | 客户真正的KPI可能是\"不出事\"而非\"技术先进\"，系统需要支撑**可量化的监管成果**（如\"本月发现XX起违规，处置率100%\"） |\n| **执法权边界** | 网信办、广电、文旅、市场监管（针对带货）、公安（针对违法）职责交叉，调研需明确**你的系统最终给谁用、谁能下处置指令** |\n| **专项行动时效性** | 针对特定主播/品类的专项整治窗口期短（如1-3个月），系统能否快速响应临时监管需求 |\n\n---\n\n## 二、数据源获取调研（核心难点）\n\n这是ToG产品最大的卡点和隐性成本所在。\n\n### 2.1 直播平台数据接口\n\n| 难点 | 具体挑战 |\n|------|----------|\n| **头部平台配合度** | 抖音、快手、淘宝、视频号、小红书是否愿意开放接口？通常是**省级以上监管单位**才有谈判筹码，市级/区县级很难直接对接 |\n| **接口标准混乱** | 各平台数据格式、字段定义、更新频率不统一，需要大量ETL适配工作 |\n| **数据延迟问题** | 平台可能延迟报送（规避实时监管），或只给回放数据而非实时流 |\n| **中小平台/野平台** | 大量长尾平台（小直播平台、境外平台、私域直播）没有开放接口，只能通过**主动采集**获取 |\n\n### 2.2 主动采集的可行性\n\n| 难点 | 具体挑战 |\n|------|----------|\n| **反爬对抗** | 主流平台有成熟的反爬机制（滑块验证、设备指纹、行为检测），采集成本高且不稳定 |\n| **私域直播盲区** | 微信视频号私域、抖音粉丝群直播、小程序直播等封闭生态难以触达 |\n| **跨境数据** | TikTok、Temu等海外直播业务的监管边界和数据获取合法性 |\n| **数据存储合规** | 采集的直播内容涉及公民个人信息，存储和使用需符合《个人信息保护法》，政府项目对此极其敏感 |\n\n### 2.3 行业数据与第三方数据\n\n| 数据类型 | 获取难点 |\n|----------|----------|\n| **MCN机构名单** | 无统一公开名录，需从各平台爬取或通过行业协会获取，数据更新滞后 |\n| **主播实名信息** | 平台实名数据与主播昵称的映射关系，涉及隐私，获取受限 |\n| **商品/供应链数据** | 带货直播中的商品信息、价格、库存、成交数据，平台不对外开放 |\n| **历史违规档案** | 各平台处罚记录不互通，主播跨平台\"换马甲\"难以识别 |\n\n---\n\n## 三、舆论监控调研（新增）\n\n| 难点 | 具体挑战 |\n|------|----------|\n| **舆情与直播的关联** | 微博上某主播的热搜，如何关联到其正在进行的直播？需要**跨平台ID映射** |\n| **实时性要求** | 舆情爆发往往在几分钟内，系统需要**分钟级**发现热点并关联直播内容 |\n| **情绪化内容识别** | 弹幕、评论中的反讽、暗示、拼音缩写（如\"zf\"\"gj\"）难以识别 |\n| **谣言溯源** | 直播切片被恶意剪辑传播，如何追踪原始直播源 |\n| **境外舆情** | Twitter/X、YouTube上的华语直播内容监控，技术和合规双重门槛 |\n\n---\n\n## 四、带货平台与主播画像（新增）\n\n| 难点 | 具体挑战 |\n|------|----------|\n| **平台碎片化** | 传统电商（淘宝、京东、拼多多）+ 内容电商（抖音、快手、小红书）+ 私域（微信社群、小程序），数据分散 |\n| **主播身份多层嵌套** | 一个自然人可能有多个账号（主号、小号、矩阵号），且与MCN、供应链、品牌方关系复杂 |\n| **动态风险画像** | 主播风险不仅看历史违规，还要看**近期带货品类**（如突然转卖保健品的主播风险升高）、**粉丝增长异常**等动态指标 |\n| **商品合规性** | 直播带货涉及假冒伪劣、虚假宣传、价格欺诈，需要打通**商品库、商标库、价格监测**等多维数据 |\n| **跨境电商特殊性** | 保税仓直播、海外直邮的监管归属（海关 vs 市场监管），数据获取渠道不同 |\n\n---\n\n## 五、其他关键调研维度（ToG特有）\n\n| 调研维度 | 难点说明 |\n|----------|----------|\n| **现有系统摸底** | 客户是否已有舆情系统（如人民在线、清博）、网安平台、视频监控平台？**你的系统是替代、补充还是对接？** |\n| **上级系统对接** | 是否需要对接国家级监管平台（如全国互联网安全管理服务平台）？接口标准是否已开放？ |\n| **决策链条与预算** | 网信办立项还是大数据局统筹？财政承受能力如何？是否有专项经费（如\"清朗\"专项资金）？ |\n| **竞品格局** | 传统安防大厂（海康、大华）、互联网大厂（阿里绿网、腾讯天御）、专业内容安全公司（知道创宇、任子行、恒安嘉新）的优劣势，**客户对供应商的偏好**（国企背景？本地企业？） |\n| **人员编制现状** | 客户现有人工审核团队规模？系统是要\"替代人\"还是\"辅助人\"？这决定产品自动化程度的定位 |\n\n---\n\n## 六、给你的关键建议（ToG视角）\n\n1. **数据获取是生死线**：先搞定一个头部平台的省级数据合作案例，哪怕是通过**驻场设备**或**专线接入**的方式，证明可行性后再谈规模化。\n\n2. **找\"样板间\"客户**：优先选择有明确痛点、愿意配合调研的市级网信办（如直播电商发达城市：杭州、广州、义务），用个案验证数据链路。\n\n3. **合规前置**：ToG项目对数据安全、等保、密评要求极高，调研阶段就需要**保密资质**和**等保咨询**介入。\n\n4. **明确产品边界**：是做**数据采集平台**（只负责汇聚数据）还是**智能研判平台**（提供违规预警）？ToG客户往往希望\"交钥匙工程\"，但建议先做专做精。\n\n---\n\n**下一步建议**：\n- 是否需要我帮你针对**某个具体省份/城市**的网信办需求做更细化的调研框架？\n- 或者重点展开**数据获取技术方案**（如何合法合规地拿到平台数据）的可行性分析？",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        let content = elements[0]["content"].as_str().expect("markdown content");
        assert!(content.contains("## 一、政策与考核指标调研（政府视角）"));
        assert!(content.contains("## 六、给你的关键建议（ToG视角）"));
    }

    #[test]
    fn build_feishu_markdown_card_splits_large_review_outline_under_markdown_element_limit() {
        let card = build_feishu_markdown_card(
            "这部分是系统的**核心技术壁垒**，也是乙方最容易**虚报能力**或**低估难度**的地方。作为甲方，你需要重点审视以下维度：\n\n---\n\n## 一、直播录制与信息采集的坑\n\n### 1.1 采集层面的技术难点\n\n| 坑点 | 乙方可能吹的 | 实际难度 | 你的审查要点 |\n|------|-------------|---------|-------------|\n| **协议兼容性** | \"支持所有主流协议RTMP/FLV/HLS\" | 各平台有**私有协议变种**（如抖音的QUIC、快手的KTP），且频繁升级 | 要求提供**具体平台清单**和**版本日期**，问\"如果平台升级导致采集失败，谁负责适配\" |\n| **反爬对抗** | \"我们有成熟采集方案\" | 主流平台有**设备指纹、行为检测、滑块验证、IP封禁** | 问清楚**IP池规模、轮换策略、验证码破解方案**，要求提供**采集成功率统计数据** |\n| **音视频同步** | \"自动录制存储\" | 高并发下音视频不同步、花屏、丢帧 | 要求**录制文件完整性校验机制**（如每段视频MD5校验、关键帧检测） |\n| **弹幕/礼物数据** | \"同时采集弹幕和礼物\" | 弹幕协议加密、礼物数据需要**逆向APP**或**Hook接口** | 明确是**官方API获取**还是**逆向破解**，后者有法律风险且不稳定 |\n\n### 1.2 采集合规性坑（ToG特别敏感）\n\n| 风险点 | 问题描述 | 乙方可能的套路 |\n|--------|---------|---------------|\n| **授权链条** | 采集直播内容涉及个人信息和版权，是否需要平台授权？ | 乙方说\"监管用途无需授权\"，但实际操作中平台可能**断流或起诉** |\n| **数据留存** | 录制内容存储涉及公民肖像、言论，存储期限和范围 | 乙方不提**脱敏策略**和**访问审计**，导致合规风险 |\n| **跨境采集** | TikTok、YouTube等海外直播的采集合法性 | 乙方可能承诺能做，但涉及**跨境数据安全审查** |\n\n**关键问题**：要求乙方明确**数据采集的法律依据**和**平台合作协议**，不接受\"技术能实现就行\"。\n\n---\n\n## 二、数据穿透（主播→MCN→公司→法人→资质）的坑\n\n这是**数据融合**的核心难点，也是乙方方案水平的试金石。\n\n### 2.1 数据穿透的技术难点\n\n| 穿透层级 | 难点 | 乙方可能的虚标 | 真实挑战 |\n|----------|------|---------------|---------|\n| **主播身份** | 昵称→实名→身份证 | \"对接平台实名接口\" | 平台**不开放实名数据**，或只给部分脱敏信息；同一主播多平台账号**关联困难** |\n| **MCN机构** | 主播所属MCN | \"自动识别MCN\" | MCN与主播关系**非公开**，需从简介、直播口播、合同备案等多源交叉验证 |\n| **公司主体** | MCN背后的公司 | \"工商数据自动关联\" | MCN常用**多层嵌套架构**（VIE、SPV），且频繁变更股权 |\n| **法人/股东** | 最终受益人(UBO) | \"穿透到实际控制人\" | 存在**代持、离岸架构、家族信托**，工商数据看不到真实控制人 |\n| **资质证照** | 营业执照、许可证 | \"自动核验资质\" | 证照有**电子版、纸质版、历史版本**，且存在**PS造假、过期、超范围经营** |\n\n### 2.2 数据源的可靠性坑\n\n| 数据源 | 乙方可能说的 | 实际问题 |\n|--------|-------------|---------|\n| **平台官方数据** | \"与平台深度合作\" | 可能只有**少量测试数据**，或平台**只给脱敏数据** |\n| **工商数据库** | \"对接国家企业信用信息公示系统\" | 实际是爬取企查查/天眼查（**数据滞后、不准确**），或买第三方接口（**费用未告知**） |\n| **资质核验** | \"自动验证资质真伪\" | 文旅部的网络文化经营许可证、市场监管的食品经营许可等**没有统一开放API**，需人工或OCR+人工复核 |\n\n### 2.3 关联分析的准确性坑\n\n| 场景 | 乙方承诺 | 实际难度 |\n|------|---------|---------|\n| **账号关联** | \"识别同一人的多个账号\" | 需**声纹识别、人脸聚类、设备指纹、行为模式分析**，技术复杂且涉及隐私 |\n| **关系图谱** | \"构建主播-MCN-公司关系图谱\" | 数据稀疏、关系动态变化，图谱**准确率可能不足60%** |\n| **风险传导** | \"MCN违规自动标记旗下主播\" | 需定义**风险传导规则**，且存在**误伤**（已解约主播仍被关联） |\n\n---\n\n## 三、如何判断乙方方案是否可行、是否厉害\n\n### 3.1 一眼识破\"假大空\"方案的红线\n\n| 乙方说辞 | 问题 | 你应该问 |\n|----------|------|---------|\n| \"我们对接了所有主流平台\" | 不可能，平台API不开放 | \"请提供**平台授权协议**或**数据接入证明**，具体到哪些字段、更新频率\" |\n| \"自动穿透到实际控制人和资质\" | UBO穿透是金融行业难题，非公开数据 | \"穿透准确率是多少？无法穿透的比例是多少？**兜底的人工核查流程**是什么\" |\n| \"AI自动关联主播所有账号\" | 账号关联技术门槛高，准确率低 | \"关联依据是什么（人脸/声纹/设备/行为）？**误关联率**怎么控制？\" |\n\n### 3.2 验证乙方真实能力的测试方法\n\n**测试1：POC穿透测试**\n- 给乙方5-10个**真实主播昵称**（涵盖头部、腰部、素人，含跨平台情况）\n- 要求乙方**现场演示**能穿透到哪一层（MCN？公司？法人？）\n- **关键看**：能否识别**隐性关联**（如主播与公司之间没有公开股权关系，但通过商标、合同、直播背景、客服电话等侧面关联）\n\n| 维度 | 普通/吹牛方案 | 高手方案 |\n|------|--------------|---------|\n| **数据来源透明度** | 只说\"大数据平台\" | 明确列出每一层数据源、更新频率、置信度 |\n| **无法识别的处理** | 直接忽略或瞎猜 | 标记**置信度**，并触发人工核查流程 |\n| **样本验证** | 只拿成功案例展示 | 同时展示**失败案例**和原因 |\n\n---\n\n## 四、给你的审查Checklist（数据采集与穿透专项）\n\n```\n□ 采集方案是否明确区分官方API/授权接入/逆向采集，并说明各自占比\n□ 是否提供采集成功率、延迟、完整性的量化指标和测试方法\n□ 数据穿透是否明确每一层的数据源（如MCN从哪来、公司从哪来）\n□ 是否提供置信度评分和人工复核机制，而不是“一刀切自动结论”\n□ 是否说明法律依据：平台授权、监管依据、个人信息处理边界\n□ 是否有真实POC案例，且能现场演示\n```",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert!(elements.len() >= 2);
        for element in elements {
            let content = element["content"].as_str().expect("markdown content");
            assert!(content.chars().count() <= FEISHU_MARKDOWN_ELEMENT_SOFT_LIMIT);
        }
    }

    #[test]
    fn build_feishu_markdown_card_preserves_complex_chinese_supplement_table() {
        let card = build_feishu_markdown_card(
            "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
             |------|----------|----------|--------------|\n\
             | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
             | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
             | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
             | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
             | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
             | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
             | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
             | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
             | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n",
        )
        .expect("card should be built");

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0]["tag"], "markdown");
        let content = elements[0]["content"].as_str().expect("markdown content");
        assert!(content.contains("| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |"));
        assert!(content.contains("| **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |"));
        assert!(content.contains(
            "| **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |"
        ));
    }

    #[test]
    fn build_feishu_markdown_card_matches_expected_supplement_table_schema() {
        let markdown = "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n\
                        |------|----------|----------|--------------|\n\
                        | **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n\
                        | **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n\
                        | **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n\
                        | **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n\
                        | **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n\
                        | **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n\
                        | **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n\
                        | **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n\
                        | **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |\n";
        let card = build_feishu_markdown_card(markdown).expect("card should be built");

        let expected = json!({
            "schema": "2.0",
            "config": {
                "update_multi": true
            },
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n| --- | --- | --- | --- |\n| **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n| **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n| **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n| **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n| **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n| **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n| **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n| **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n| **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |",
                        "text_align": "left"
                    }
                ]
            }
        });

        assert_eq!(card, expected);
    }

    #[test]
    fn build_feishu_interactive_payload_serializes_card_into_content_string() {
        let card = json!({
            "schema": "2.0",
            "config": {
                "update_multi": true
            },
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": "**bold**",
                        "text_align": "left"
                    }
                ]
            }
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload["msg_type"], "interactive");
        assert!(payload.get("card").is_none());

        let content = payload["content"]
            .as_str()
            .expect("interactive content should be a serialized JSON string");
        let reparsed: Value =
            serde_json::from_str(content).expect("interactive content should parse back to JSON");
        assert_eq!(reparsed, card);
    }

    #[test]
    fn build_feishu_interactive_payload_matches_expected_reply_body_for_supplement_table() {
        let card = json!({
            "schema": "2.0",
            "config": {
                "update_multi": true
            },
            "body": {
                "elements": [
                    {
                        "tag": "markdown",
                        "content": "| 补剂 | 证据强度 | 推荐剂量 | 关键注意事项 |\n| --- | --- | --- | --- |\n| **圣约翰草** | ⭐⭐⭐ 最强 | 900mg/日 (分3次) | ⚠️与避孕药、抗凝药、抗抑郁药严重冲突；孕妇禁用 |\n| **SAM-e** | ⭐⭐⭐ 强 | 800-1600mg/日 | ⚠️双相患者慎用（诱发躁狂）；与SSRI同服有风险 |\n| **EPA鱼油** | ⭐⭐ 中等 | EPA 1-2g/日 | ⚠️与阿司匹林/华法林同服增加出血风险 |\n| **藏红花** | ⭐⭐ 中等 | 30mg/日 | ⚠️孕妇禁用 |\n| **维生素D** | ⭐⭐ 缺乏者有效 | 1000-4000 IU/日 | 建议先检测水平再补充 |\n| **L-甲基叶酸** | ⭐⭐ 增效剂 | 7.5-15mg/日 | 配合抗抑郁药使用效果更佳 |\n| **NAC** | ⭐⭐ 辅助 | 2000mg/日 | 哮喘患者慎用 |\n| **锌** | ⭐ 初步 | 25-50mg/日 | 长期高剂量导致铜缺乏 |\n| **5-HTP** | ⭐ 有限 | 100-300mg/日 | ⚠️与抗抑郁药同服有血清素综合征风险 |",
                        "text_align": "left"
                    }
                ]
            }
        });

        let expected_payload = json!({
            "msg_type": "interactive",
            "content": card.to_string()
        });

        let payload = build_feishu_interactive_payload(&card);
        assert_eq!(payload, expected_payload);
    }

    #[test]
    fn build_ask_user_question_card_renders_single_and_multi_select_fields() {
        let card = build_ask_user_question_card(&AskUserQuestionRequestEvent {
            request_id: "req-1".to_string(),
            conversation_id: Some(42),
            questions: vec![
                AskUserQuestionItem {
                    question: "选择一个模型".to_string(),
                    header: "模型".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "GPT-5.4".to_string(),
                            description: "推荐".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "Claude".to_string(),
                            description: "保守".to_string(),
                        },
                    ],
                    multi_select: false,
                },
                AskUserQuestionItem {
                    question: "选择输出格式".to_string(),
                    header: "格式".to_string(),
                    options: vec![
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "表格".to_string(),
                            description: "结构化".to_string(),
                        },
                        crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                            label: "列表".to_string(),
                            description: "简洁".to_string(),
                        },
                    ],
                    multi_select: true,
                },
            ],
            metadata: None,
        });

        let elements = card["body"]["elements"].as_array().expect("elements should be an array");
        let form =
            elements.iter().find(|element| element["tag"] == "form").expect("form should exist");
        let form_elements = form["elements"].as_array().expect("form elements should be array");
        assert!(form_elements.iter().any(|element| element["tag"] == "select_static"));
        assert!(form_elements.iter().any(|element| element["tag"] == "multi_select_static"));
        assert_eq!(form["name"], "ask_user_req-1");
        let submit_button = form_elements.last().expect("submit button should exist");
        assert_eq!(submit_button["tag"], "button");
        assert_eq!(submit_button["name"], "ask_user_submit");
        assert_eq!(submit_button["form_action_type"], "submit");
        assert_eq!(submit_button["behaviors"][0]["type"], "callback");
        assert_eq!(submit_button["behaviors"][0]["value"]["action"], "submit");
        assert_eq!(submit_button["behaviors"][0]["value"]["request_id"], "req-1");

        let cancel_button = elements
            .iter()
            .find(|element| element["name"] == "ask_user_cancel")
            .expect("cancel button should exist");
        assert_eq!(cancel_button["tag"], "button");
        assert_eq!(cancel_button["name"], "ask_user_cancel");
        assert_eq!(cancel_button["behaviors"][0]["type"], "callback");
        assert_eq!(cancel_button["behaviors"][0]["value"]["action"], "cancel");
        assert_eq!(cancel_button["behaviors"][0]["value"]["request_id"], "req-1");
    }

    #[test]
    fn parse_permission_reply_command_supports_operation_variants() {
        assert_eq!(
            parse_permission_reply_command("批准一次 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow",
            })
        );
        assert_eq!(
            parse_permission_reply_command("本任务批准 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_conversation",
            })
        );
        assert_eq!(
            parse_permission_reply_command("助手允许 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "allow_for_assistant",
            })
        );
        assert_eq!(
            parse_permission_reply_command("拒绝 OP-ABC123"),
            Some(PermissionReplyCommand::Operation {
                review_code: "OP-ABC123".to_string(),
                decision: "deny",
            })
        );
    }

    #[test]
    fn parse_permission_reply_command_supports_acp_variants() {
        assert_eq!(
            parse_permission_reply_command("批准 2 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpSelect {
                review_code: "ACP-QWERTY".to_string(),
                option_index: 2,
            })
        );
        assert_eq!(
            parse_permission_reply_command("取消 ACP-QWERTY"),
            Some(PermissionReplyCommand::AcpCancel { review_code: "ACP-QWERTY".to_string() })
        );
        assert_eq!(parse_permission_reply_command("批准 0 ACP-QWERTY"), None);
    }

    #[test]
    fn map_ask_user_form_values_to_answers_supports_single_and_multi_select() {
        let questions = vec![
            AskUserQuestionItem {
                question: "选择一个模型".to_string(),
                header: "模型".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "GPT-5.4".to_string(),
                        description: "推荐".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "Claude".to_string(),
                        description: "保守".to_string(),
                    },
                ],
                multi_select: false,
            },
            AskUserQuestionItem {
                question: "选择输出格式".to_string(),
                header: "格式".to_string(),
                options: vec![
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "表格".to_string(),
                        description: "结构化".to_string(),
                    },
                    crate::mcp::builtin_mcp::interaction::AskUserQuestionOption {
                        label: "列表".to_string(),
                        description: "简洁".to_string(),
                    },
                ],
                multi_select: true,
            },
        ];
        let form_value = Map::from_iter([
            ("question_0".to_string(), Value::String("GPT-5.4".to_string())),
            (
                "question_1".to_string(),
                Value::Array(vec![
                    Value::String("表格".to_string()),
                    Value::String("列表".to_string()),
                ]),
            ),
        ]);

        let answers = map_ask_user_form_values_to_answers(&questions, &form_value)
            .expect("answers should map");
        assert_eq!(answers.get("选择一个模型"), Some(&"GPT-5.4".to_string()));
        assert_eq!(answers.get("选择输出格式"), Some(&"表格, 列表".to_string()));
    }

    #[test]
    fn feishu_card_action_callback_parses_inner_event_payload() {
        let raw_event = json!({
            "operator": {
                "open_id": "ou_test_user"
            },
            "context": {
                "open_message_id": "om_test_message"
            },
            "action": {
                "value": {
                    "request_id": "req-1",
                    "action": "submit"
                },
                "form_value": {
                    "question_0": "GPT-5.4"
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("inner event payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn feishu_card_action_callback_parses_enveloped_payload() {
        let raw_event = json!({
            "event": {
                "operator": {
                    "open_id": "ou_test_user"
                },
                "context": {
                    "open_message_id": "om_test_message"
                },
                "action": {
                    "value": {
                        "request_id": "req-1",
                        "action": "submit"
                    },
                    "form_value": {
                        "question_0": "GPT-5.4"
                    }
                }
            }
        });

        let callback: FeishuCardActionCallback =
            serde_json::from_value(raw_event).expect("enveloped payload should parse");

        assert_eq!(callback.event().operator.open_id, "ou_test_user");
        assert_eq!(
            callback
                .event()
                .context
                .as_ref()
                .and_then(|context| context.open_message_id.as_deref()),
            Some("om_test_message")
        );
        assert_eq!(
            callback
                .event()
                .action
                .value
                .as_ref()
                .and_then(|value| value.get("request_id"))
                .and_then(Value::as_str),
            Some("req-1")
        );
    }

    #[test]
    fn find_latest_recoverable_ask_user_tool_call_prefers_pending_or_executing() {
        let base = crate::db::mcp_db::MCPToolCall {
            id: 1,
            conversation_id: 42,
            message_id: None,
            subtask_id: None,
            server_id: 1,
            server_name: "ui_interaction".to_string(),
            tool_name: "ask_user_question".to_string(),
            parameters: "{}".to_string(),
            status: "success".to_string(),
            result: None,
            error: None,
            created_time: "2026-03-18 00:00:00".to_string(),
            started_time: None,
            finished_time: None,
            llm_call_id: None,
            assistant_message_id: None,
        };
        let calls = vec![
            crate::db::mcp_db::MCPToolCall {
                id: 2,
                status: "executing".to_string(),
                ..base.clone()
            },
            crate::db::mcp_db::MCPToolCall {
                id: 3,
                tool_name: "preview_file".to_string(),
                status: "pending".to_string(),
                ..base.clone()
            },
        ];

        let tool_call =
            find_latest_recoverable_ask_user_tool_call(&calls).expect("tool call should exist");
        assert_eq!(tool_call.id, 2);
    }

    #[test]
    fn parse_bot_menu_click_event_extracts_open_id_and_event_key() {
        let raw_event = json!({
            "operator": {
                "operator_id": {
                    "open_id": "ou_test_user"
                }
            },
            "event_key": "feishu::conversation::new",
            "timestamp": 1669364458
        });

        let event = parse_bot_menu_click_event(&raw_event)
            .expect("menu event should parse")
            .expect("menu event should not be empty");

        assert_eq!(
            event,
            FeishuBotMenuClickEvent {
                operator_open_id: "ou_test_user".to_string(),
                event_key: "feishu::conversation::new".to_string(),
            }
        );
    }

    #[test]
    fn feishu_relay_waits_for_finished_assistant_messages() {
        let now = Utc::now();
        let streaming = crate::db::conversation_db::Message {
            id: 1,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "半句输出".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: None,
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        };
        let finished =
            crate::db::conversation_db::Message { finish_time: Some(now), ..streaming.clone() };
        let tool_result = crate::db::conversation_db::Message {
            message_type: "tool_result".to_string(),
            finish_time: None,
            ..streaming.clone()
        };

        assert!(!is_message_ready_for_feishu_relay(&streaming));
        assert!(is_message_ready_for_feishu_relay(&finished));
        assert!(is_message_ready_for_feishu_relay(&tool_result));
    }

    #[test]
    fn debug_resend_prefers_preview_tool_result_after_response() {
        let now = Utc::now();
        let response = crate::db::conversation_db::Message {
            id: 10,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "好的。<!-- MCP_TOOL_CALL:{\"server_name\":\"UI交互工具\",\"tool_name\":\"preview_file\",\"parameters\":\"{\\\"files\\\":[{\\\"title\\\":\\\"华容道情节构思\\\"}]}\"} -->".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: Some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some("group-1".to_string()),
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        };
        let preview_tool_result = crate::db::conversation_db::Message {
            id: 11,
            message_type: "tool_result".to_string(),
            content: "Tool execution completed:\n\nTool Call ID: call_1\nTool: preview_file\nServer: UI交互工具\nParameters: {\"files\":[{\"title\":\"华容道情节构思\",\"type\":\"text\",\"content\":\"完整正文\"}]}\nResult:\n[{\"type\":\"json\",\"json\":{\"status\":\"preview_shown\"}}]".to_string(),
            ..response.clone()
        };
        let later_response = crate::db::conversation_db::Message {
            id: 12,
            content: "后续回复".to_string(),
            ..response.clone()
        };

        let selected = collect_feishu_debug_resend_messages(
            &response,
            &[response.clone(), preview_tool_result.clone(), later_response],
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, preview_tool_result.id);
    }

    #[test]
    fn preview_tool_result_rendering_keeps_full_content_from_tool_result() {
        let now = Utc::now();
        let preview_tool_result = crate::db::conversation_db::Message {
            id: 11,
            parent_id: None,
            conversation_id: 1,
            message_type: "tool_result".to_string(),
            content: "Tool execution completed:\n\nTool Call ID: call_1\nTool: preview_file\nServer: UI交互工具\nParameters: {\"files\":[{\"title\":\"华容道情节构思\",\"type\":\"text\",\"content\":\"完整正文\"}]}\nResult:\n[{\"type\":\"json\",\"json\":{\"status\":\"preview_shown\"}}]".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: Some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some("group-1".to_string()),
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        };

        assert!(preview_tool_result_has_inline_content(&preview_tool_result));
        let rendered =
            render_inline_preview_tool_result_parts_for_feishu(&preview_tool_result, "aipp")
                .expect("inline preview content should render");

        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("华容道情节构思"));
        assert!(rendered[0].contains("完整正文"));
    }

    #[test]
    fn preview_response_parameters_are_extracted_from_mcp_comment() {
        let now = Utc::now();
        let response = crate::db::conversation_db::Message {
            id: 12,
            parent_id: None,
            conversation_id: 1,
            message_type: "response".to_string(),
            content: "修改完成。\n\n<!-- MCP_TOOL_CALL:{\"call_id\":1644,\"llm_call_id\":\"functions.UI__preview_file:5\",\"parameters\":\"{\\\"files\\\":[{\\\"title\\\":\\\"第二章样稿.md\\\",\\\"type\\\":\\\"text\\\",\\\"url\\\":\\\"D:\\\\\\\\BaiduSyncdisk\\\\\\\\ai_area\\\\\\\\01-进行中的项目\\\\\\\\三国-侠之大者\\\\\\\\05-草稿区\\\\\\\\第二章样稿.md\\\"}]}\",\"server_name\":\"UI交互工具\",\"tool_name\":\"preview_file\"} -->".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: Some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some("group-1".to_string()),
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        };

        let params = preview_file_parameters_from_response_message(&response)
            .expect("preview_file response params");

        assert_eq!(params["files"][0]["title"], "第二章样稿.md");
        assert_eq!(params["files"][0]["type"], "text");
        assert_eq!(
            params["files"][0]["url"],
            "D:\\BaiduSyncdisk\\ai_area\\01-进行中的项目\\三国-侠之大者\\05-草稿区\\第二章样稿.md"
        );
    }

    #[test]
    fn preview_tool_result_with_only_local_url_is_not_treated_as_inline_content() {
        let now = Utc::now();
        let preview_tool_result = crate::db::conversation_db::Message {
            id: 12,
            parent_id: None,
            conversation_id: 1,
            message_type: "tool_result".to_string(),
            content: "Tool execution completed:\n\nTool Call ID: call_2\nTool: preview_file\nServer: UI交互工具\nParameters: {\"files\":[{\"title\":\"第一章样稿\",\"type\":\"markdown\",\"url\":\"D:\\\\demo\\\\chapter1.md\"}]}\nResult:\n[{\"type\":\"json\",\"json\":{\"status\":\"preview_shown\"}}]".to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: Some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: Some("group-1".to_string()),
            parent_group_id: None,
            tool_calls_json: None,
            metadata_json: None,
            first_token_time: None,
            ttft_ms: None,
        };

        assert!(!preview_tool_result_has_inline_content(&preview_tool_result));
        assert!(render_inline_preview_tool_result_parts_for_feishu(&preview_tool_result, "aipp")
            .is_none());
    }
}
