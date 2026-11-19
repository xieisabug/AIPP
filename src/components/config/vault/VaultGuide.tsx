import { KeyRound, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";

const steps = [
    {
        title: "存进来",
        text: "新建条目，填入 API Key、密码或身份证号。值加密保存，之后不再显示。",
    },
    {
        title: "让助手用",
        text: (
            <>
                助手写 <code className="font-mono">{"<vault:键>"}</code> 代替真实的值；也可以在 Provider、MCP 配置里点
                <KeyRound className="mx-0.5 inline h-3 w-3 align-[-1px]" aria-label="钥匙" />
                选择它。
            </>
        ),
    },
    {
        title: "由你批准",
        text: "每次使用前都会先问你。常用的网站可以设为自动允许。",
    },
];

/** 保险箱为空时的说明：三步用法 + 新建入口。children 放同步提醒等附加信息。 */
export default function VaultGuide({ onCreate, children }: { onCreate: () => void; children?: React.ReactNode }) {
    return (
        <div className="mx-auto max-w-3xl space-y-8 py-6" data-aipp-slot="vault-guide">
            <h2 className="text-center text-xl font-semibold">把密码、API Key 交给助手用，但不让它看到</h2>
            <ol className="grid gap-3 sm:grid-cols-3">
                {steps.map((step, index) => (
                    <li key={step.title} className="space-y-2 rounded-lg border border-border bg-card p-4" data-aipp-slot="vault-guide-step">
                        <div className="flex items-center gap-2">
                            <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-medium tabular-nums">{index + 1}</span>
                            <span className="text-sm font-medium">{step.title}</span>
                        </div>
                        <p className="text-xs leading-relaxed text-muted-foreground">{step.text}</p>
                    </li>
                ))}
            </ol>
            <div className="space-y-2 text-center">
                <Button onClick={onCreate} className="gap-2">
                    <Plus className="h-4 w-4" />
                    新建条目
                </Button>
                <p className="text-xs text-muted-foreground">也可以等助手需要时，在对话里当场填写。</p>
            </div>
            {children}
        </div>
    );
}
