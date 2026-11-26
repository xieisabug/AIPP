import { useEffect, useState } from "react";

const STORAGE_KEY = "aipp:review-model-hint-seen";

/** Only consume the hint when a manual confirmation card actually mounts. */
export function ReviewModelHint() {
    const [visible, setVisible] = useState(false);
    useEffect(() => {
        if (!localStorage.getItem(STORAGE_KEY)) {
            localStorage.setItem(STORAGE_KEY, "1");
            setVisible(true);
        }
    }, []);
    return visible ? <p className="mt-2 text-xs text-muted-foreground">配置审核模型后，安全的操作会自动通过</p> : null;
}
