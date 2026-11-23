export interface AppUpdateStatus {
    revision: number;
    phase: "idle" | "checking" | "up_to_date" | "downloading" | "ready" | "installing" | "installed" | "failed";
    version: string | null;
    body: string | null;
    downloaded: number;
    total: number | null;
    error: string | null;
}
