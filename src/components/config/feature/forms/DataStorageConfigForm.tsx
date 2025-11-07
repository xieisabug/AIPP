import React, { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Database, HardDrive, Cloud } from "lucide-react";
import { toast } from "sonner";

export const DataStorageConfigForm: React.FC = () => {
    const [activeTab, setActiveTab] = useState<string>("local");
    const [remoteTab, setRemoteTab] = useState<string>("supabase");

    // Local storage handlers
    const handleOpenDataFolder = useCallback(() => {
        invoke("open_data_folder");
    }, []);

    const handleSaveLocalConfig = useCallback(async () => {
        try {
            await invoke("save_data_storage_config", {
                storageMode: "local",
                storage_mode: "local", // 兼容 snake_case
                remoteType: null,
                remote_type: null,
                payload: {},
            });
            toast.success("已切换为本地存储并保存配置");
        } catch (e: any) {
            toast.error("保存失败: " + (e?.toString?.() ?? e));
        }
    }, []);

    // Supabase handlers
    const [supabaseUrl, setSupabaseUrl] = useState("");
    const [supabaseKey, setSupabaseKey] = useState("");
    const [supabaseDbHost, setSupabaseDbHost] = useState("");
    const [supabaseDbPassword, setSupabaseDbPassword] = useState("");
    const [uploadProgress, setUploadProgress] = useState("");

    const handleSaveSupabaseConfig = useCallback(async () => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请填写 Supabase URL 和 Anon Key");
            return;
        }
        try {
            const payload: Record<string, string> = {
                supabase_url: supabaseUrl,
                supabase_key: supabaseKey,
            };
            // 可选的数据库连接信息
            if (supabaseDbHost) payload.supabase_db_host = supabaseDbHost;
            if (supabaseDbPassword) payload.supabase_db_password = supabaseDbPassword;

            await invoke("save_data_storage_config", {
                storageMode: "remote",
                storage_mode: "remote",
                remoteType: "supabase",
                remote_type: "supabase",
                payload,
            });
            toast.success("Supabase 配置已保存");
        } catch (e: any) {
            toast.error("保存失败: " + (e?.toString?.() ?? e));
        }
    }, [supabaseUrl, supabaseKey, supabaseDbHost, supabaseDbPassword]);

    const handleTestSupabase = useCallback(async () => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请先填写完整的 Supabase 配置");
            return;
        }
        try {
            await invoke("test_remote_storage_connection", {
                remoteType: "supabase",
                remote_type: "supabase",
                payload: {
                    supabase_url: supabaseUrl,
                    supabase_key: supabaseKey,
                },
            });
            toast.success("Supabase 连接正常");
        } catch (e: any) {
            toast.error("连接失败: " + (e?.toString?.() ?? e));
        }
    }, [supabaseUrl, supabaseKey]);

    const handleUploadSupabase = useCallback(async () => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请先填写 Supabase URL 和 Anon Key");
            return;
        }
        if (!supabaseDbHost || !supabaseDbPassword) {
            toast.error("上传数据需要填写数据库连接信息（数据库主机地址和密码）。\n\n在 Supabase 项目设置 → Database → Connection string 中可以找到这些信息。", {
                duration: 6000,
            });
            return;
        }

        setUploadProgress("开始上传...");

        // 监听上传进度
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<any>("upload-progress", (event) => {
            const { stage, message } = event.payload;
            setUploadProgress(message);
            if (stage === "completed") {
                toast.success("数据上传完成！");
            }
        });

        try {
            await invoke("upload_local_data", {
                remoteType: "supabase",
                remote_type: "supabase",
                payload: {
                    supabase_url: supabaseUrl,
                    supabase_key: supabaseKey,
                    supabase_db_host: supabaseDbHost,
                    supabase_db_password: supabaseDbPassword,
                },
            });
        } catch (e: any) {
            toast.error("上传失败: " + (e?.toString?.() ?? e));
            setUploadProgress("");
        } finally {
            unlisten();
        }
    }, [supabaseUrl, supabaseKey, supabaseDbHost, supabaseDbPassword]);

    // PostgreSQL handlers
    const [pgHost, setPgHost] = useState("");
    const [pgPort, setPgPort] = useState("5432");
    const [pgDatabase, setPgDatabase] = useState("");
    const [pgUsername, setPgUsername] = useState("");
    const [pgPassword, setPgPassword] = useState("");

    const handleSavePostgresConfig = useCallback(async () => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请填写完整的 PostgreSQL 配置");
            return;
        }
        try {
            await invoke("save_data_storage_config", {
                storageMode: "remote",
                storage_mode: "remote",
                remoteType: "postgresql",
                remote_type: "postgresql",
                payload: {
                    pg_host: pgHost,
                    pg_port: pgPort,
                    pg_database: pgDatabase,
                    pg_username: pgUsername,
                    pg_password: pgPassword,
                },
            });
            toast.success("PostgreSQL 配置已保存");
        } catch (e: any) {
            toast.error("保存失败: " + (e?.toString?.() ?? e));
        }
    }, [pgHost, pgPort, pgDatabase, pgUsername, pgPassword]);

    const handleTestPostgres = useCallback(async () => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请先填写完整的 PostgreSQL 配置");
            return;
        }
        try {
            await invoke("test_remote_storage_connection", {
                remoteType: "postgresql",
                remote_type: "postgresql",
                payload: {
                    pg_host: pgHost,
                    pg_port: pgPort,
                    pg_database: pgDatabase,
                    pg_username: pgUsername,
                    pg_password: pgPassword,
                },
            });
            toast.success("PostgreSQL 连接正常");
        } catch (e: any) {
            toast.error("连接失败: " + (e?.toString?.() ?? e));
        }
    }, [pgHost, pgPort, pgDatabase, pgUsername, pgPassword]);

    const handleUploadPostgres = useCallback(async () => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请先填写完整的 PostgreSQL 配置");
            return;
        }

        setUploadProgress("开始上传...");

        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<any>("upload-progress", (event) => {
            const { stage, message } = event.payload;
            setUploadProgress(message);
            if (stage === "completed") {
                toast.success("数据上传完成！");
            }
        });

        try {
            await invoke("upload_local_data", {
                remoteType: "postgresql",
                remote_type: "postgresql",
                payload: {
                    pg_host: pgHost,
                    pg_port: pgPort,
                    pg_database: pgDatabase,
                    pg_username: pgUsername,
                    pg_password: pgPassword,
                },
            });
        } catch (e: any) {
            toast.error("上传失败: " + (e?.toString?.() ?? e));
            setUploadProgress("");
        } finally {
            unlisten();
        }
    }, [pgHost, pgPort, pgDatabase, pgUsername, pgPassword]);

    // MySQL handlers
    const [mysqlHost, setMysqlHost] = useState("");
    const [mysqlPort, setMysqlPort] = useState("3306");
    const [mysqlDatabase, setMysqlDatabase] = useState("");
    const [mysqlUsername, setMysqlUsername] = useState("");
    const [mysqlPassword, setMysqlPassword] = useState("");

    const handleSaveMysqlConfig = useCallback(async () => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请填写完整的 MySQL 配置");
            return;
        }
        try {
            await invoke("save_data_storage_config", {
                storageMode: "remote",
                storage_mode: "remote",
                remoteType: "mysql",
                remote_type: "mysql",
                payload: {
                    mysql_host: mysqlHost,
                    mysql_port: mysqlPort,
                    mysql_database: mysqlDatabase,
                    mysql_username: mysqlUsername,
                    mysql_password: mysqlPassword,
                },
            });
            toast.success("MySQL 配置已保存");
        } catch (e: any) {
            toast.error("保存失败: " + (e?.toString?.() ?? e));
        }
    }, [mysqlHost, mysqlPort, mysqlDatabase, mysqlUsername, mysqlPassword]);

    const handleTestMysql = useCallback(async () => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请先填写完整的 MySQL 配置");
            return;
        }
        try {
            await invoke("test_remote_storage_connection", {
                remoteType: "mysql",
                remote_type: "mysql",
                payload: {
                    mysql_host: mysqlHost,
                    mysql_port: mysqlPort,
                    mysql_database: mysqlDatabase,
                    mysql_username: mysqlUsername,
                    mysql_password: mysqlPassword,
                },
            });
            toast.success("MySQL 连接正常");
        } catch (e: any) {
            toast.error("连接失败: " + (e?.toString?.() ?? e));
        }
    }, [mysqlHost, mysqlPort, mysqlDatabase, mysqlUsername, mysqlPassword]);

    const handleUploadMysql = useCallback(async () => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请先填写完整的 MySQL 配置");
            return;
        }

        setUploadProgress("开始上传...");

        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<any>("upload-progress", (event) => {
            const { stage, message } = event.payload;
            setUploadProgress(message);
            if (stage === "completed") {
                toast.success("数据上传完成！");
            }
        });

        try {
            await invoke("upload_local_data", {
                remoteType: "mysql",
                remote_type: "mysql",
                payload: {
                    mysql_host: mysqlHost,
                    mysql_port: mysqlPort,
                    mysql_database: mysqlDatabase,
                    mysql_username: mysqlUsername,
                    mysql_password: mysqlPassword,
                },
            });
        } catch (e: any) {
            toast.error("上传失败: " + (e?.toString?.() ?? e));
            setUploadProgress("");
        } finally {
            unlisten();
        }
    }, [mysqlHost, mysqlPort, mysqlDatabase, mysqlUsername, mysqlPassword]);

    // 初始化加载已保存配置并反显
    useEffect(() => {
        (async () => {
            try {
                const saved: Record<string, string> = await invoke("get_data_storage_config");
                // storage_mode / storageMode 兼容
                const mode = saved.storage_mode || saved.storageMode || "local";
                if (mode === "remote") {
                    setActiveTab("remote");
                } else {
                    setActiveTab("local");
                }
                const remoteType = saved.remote_type || saved.remoteType || "supabase";
                setRemoteTab(remoteType);

                // Supabase
                if (remoteType === "supabase") {
                    if (saved.supabase_url) setSupabaseUrl(saved.supabase_url);
                    if (saved.supabase_key) setSupabaseKey(saved.supabase_key);
                    if (saved.supabase_db_host) setSupabaseDbHost(saved.supabase_db_host);
                    if (saved.supabase_db_password) setSupabaseDbPassword(saved.supabase_db_password);
                }
                // PostgreSQL
                if (remoteType === "postgresql") {
                    if (saved.pg_host) setPgHost(saved.pg_host);
                    if (saved.pg_port) setPgPort(saved.pg_port);
                    if (saved.pg_database) setPgDatabase(saved.pg_database);
                    if (saved.pg_username) setPgUsername(saved.pg_username);
                    if (saved.pg_password) setPgPassword(saved.pg_password);
                }
                // MySQL
                if (remoteType === "mysql") {
                    if (saved.mysql_host) setMysqlHost(saved.mysql_host);
                    if (saved.mysql_port) setMysqlPort(saved.mysql_port);
                    if (saved.mysql_database) setMysqlDatabase(saved.mysql_database);
                    if (saved.mysql_username) setMysqlUsername(saved.mysql_username);
                    if (saved.mysql_password) setMysqlPassword(saved.mysql_password);
                }
            } catch (e: any) {
                // 忽略未保存或命令缺失错误
                console.debug("加载数据存储配置失败", e);
            }
        })();
    }, []);

    return (
        <Card className="shadow-md hover:shadow-lg transition-shadow border-l-4 border-l-primary">
            <CardHeader>
                <CardTitle className="text-lg font-semibold text-foreground flex items-center gap-2">
                    <Database className="h-5 w-5" />
                    数据存储
                </CardTitle>
                <CardDescription className="text-sm text-muted-foreground">
                    配置本地或远程数据存储方式
                </CardDescription>
            </CardHeader>

            <CardContent>
                <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
                    <TabsList className="grid w-full grid-cols-2">
                        <TabsTrigger value="local" className="flex items-center gap-2">
                            <HardDrive className="h-4 w-4" />
                            本地
                        </TabsTrigger>
                        <TabsTrigger value="remote" className="flex items-center gap-2">
                            <Cloud className="h-4 w-4" />
                            远程
                        </TabsTrigger>
                    </TabsList>

                    {/* Local Storage Tab */}
                    <TabsContent value="local" className="space-y-4 mt-4">
                        <div className="space-y-4">
                            <div className="flex items-center justify-between p-4 bg-muted/50 rounded-lg">
                                <div className="flex-1">
                                    <Label className="font-medium text-foreground">数据文件夹</Label>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        打开本地数据存储目录
                                    </p>
                                </div>
                                <Button
                                    variant="outline"
                                    onClick={handleOpenDataFolder}
                                    className="hover:bg-muted hover:border-border"
                                >
                                    打开
                                </Button>
                            </div>

                            <div className="flex items-center justify-between p-4 bg-muted/50 rounded-lg">
                                <div className="flex-1">
                                    <Label className="font-medium text-foreground">远程数据同步</Label>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        从远程服务器同步数据（暂未实现）
                                    </p>
                                </div>
                                <Button
                                    variant="outline"
                                    onClick={() => toast.info("暂未实现，敬请期待")}
                                    className="hover:bg-muted hover:border-border"
                                >
                                    同步
                                </Button>
                            </div>

                            <div className="pt-4 border-t border-border">
                                <Button
                                    onClick={handleSaveLocalConfig}
                                    className="bg-primary hover:bg-primary/90 text-primary-foreground"
                                >
                                    保存配置
                                </Button>
                            </div>
                        </div>
                    </TabsContent>

                    {/* Remote Storage Tab */}
                    <TabsContent value="remote" className="space-y-4 mt-4">
                        <Tabs value={remoteTab} onValueChange={setRemoteTab} className="w-full">
                            <TabsList className="grid w-full grid-cols-3">
                                <TabsTrigger value="supabase">Supabase</TabsTrigger>
                                <TabsTrigger value="postgresql">PostgreSQL</TabsTrigger>
                                <TabsTrigger value="mysql">MySQL</TabsTrigger>
                            </TabsList>

                            {/* Supabase Configuration */}
                            <TabsContent value="supabase" className="space-y-4 mt-4">
                                <div className="space-y-4">
                                    <div className="space-y-3">
                                        <div className="space-y-2">
                                            <Label htmlFor="supabase-url" className="font-semibold text-sm text-foreground">
                                                Supabase URL
                                            </Label>
                                            <Input
                                                id="supabase-url"
                                                type="text"
                                                placeholder="https://xxx.supabase.co"
                                                value={supabaseUrl}
                                                onChange={(e) => setSupabaseUrl(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="supabase-key" className="font-semibold text-sm text-foreground">
                                                Supabase Anon Key
                                            </Label>
                                            <Input
                                                id="supabase-key"
                                                type="password"
                                                placeholder="输入您的 Supabase Anon Key"
                                                value={supabaseKey}
                                                onChange={(e) => setSupabaseKey(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="p-3 bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-800 rounded-lg">
                                            <p className="text-sm text-amber-700 dark:text-amber-300">
                                                <strong>注意：</strong>上传本地数据需要数据库直连信息。
                                            </p>
                                            <p className="text-xs text-amber-600 dark:text-amber-400 mt-1">
                                                在 Supabase 控制台：Project Settings → Database → Connection string 中，选择 "URI" 格式，可以看到类似：<br />
                                                <code className="bg-amber-100 dark:bg-amber-900 px-1 py-0.5 rounded">
                                                    postgresql://postgres:[YOUR-PASSWORD]@db.xxxxx.supabase.co:5432/postgres
                                                </code><br />
                                                从中提取主机地址（如 db.xxxxx.supabase.co）和密码。
                                            </p>
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="supabase-db-host" className="font-semibold text-sm text-foreground">
                                                数据库主机地址 <span className="text-muted-foreground text-xs">(上传数据时需要)</span>
                                            </Label>
                                            <Input
                                                id="supabase-db-host"
                                                type="text"
                                                placeholder="db.xxx.supabase.co（可选）"
                                                value={supabaseDbHost}
                                                onChange={(e) => setSupabaseDbHost(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="supabase-db-password" className="font-semibold text-sm text-foreground">
                                                数据库密码 <span className="text-muted-foreground text-xs">(上传数据时需要)</span>
                                            </Label>
                                            <Input
                                                id="supabase-db-password"
                                                type="password"
                                                placeholder="输入数据库密码（可选）"
                                                value={supabaseDbPassword}
                                                onChange={(e) => setSupabaseDbPassword(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>
                                    </div>

                                    {uploadProgress && (
                                        <div className="p-3 bg-blue-50 dark:bg-blue-950 border border-blue-200 dark:border-blue-800 rounded-lg">
                                            <p className="text-sm text-blue-700 dark:text-blue-300">{uploadProgress}</p>
                                        </div>
                                    )}

                                    <div className="pt-4 border-t border-border flex gap-2">
                                        <Button
                                            onClick={handleTestSupabase}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                        >
                                            测试
                                        </Button>
                                        <Button
                                            onClick={handleUploadSupabase}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                            disabled={!!uploadProgress}
                                        >
                                            上传本地数据
                                        </Button>
                                        <Button
                                            onClick={handleSaveSupabaseConfig}
                                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                                        >
                                            保存配置
                                        </Button>
                                    </div>
                                </div>
                            </TabsContent>

                            {/* PostgreSQL Configuration */}
                            <TabsContent value="postgresql" className="space-y-4 mt-4">
                                <div className="space-y-4">
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor="pg-host" className="font-semibold text-sm text-foreground">
                                                主机地址
                                            </Label>
                                            <Input
                                                id="pg-host"
                                                type="text"
                                                placeholder="localhost 或 IP 地址"
                                                value={pgHost}
                                                onChange={(e) => setPgHost(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="pg-port" className="font-semibold text-sm text-foreground">
                                                端口
                                            </Label>
                                            <Input
                                                id="pg-port"
                                                type="text"
                                                placeholder="5432"
                                                value={pgPort}
                                                onChange={(e) => setPgPort(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>
                                    </div>

                                    <div className="space-y-2">
                                        <Label htmlFor="pg-database" className="font-semibold text-sm text-foreground">
                                            数据库名称
                                        </Label>
                                        <Input
                                            id="pg-database"
                                            type="text"
                                            placeholder="输入数据库名称"
                                            value={pgDatabase}
                                            onChange={(e) => setPgDatabase(e.target.value)}
                                            className="focus:ring-ring/20 focus:border-ring"
                                        />
                                    </div>

                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor="pg-username" className="font-semibold text-sm text-foreground">
                                                用户名
                                            </Label>
                                            <Input
                                                id="pg-username"
                                                type="text"
                                                placeholder="输入用户名"
                                                value={pgUsername}
                                                onChange={(e) => setPgUsername(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="pg-password" className="font-semibold text-sm text-foreground">
                                                密码
                                            </Label>
                                            <Input
                                                id="pg-password"
                                                type="password"
                                                placeholder="输入密码"
                                                value={pgPassword}
                                                onChange={(e) => setPgPassword(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>
                                    </div>

                                    {uploadProgress && (
                                        <div className="p-3 bg-blue-50 dark:bg-blue-950 border border-blue-200 dark:border-blue-800 rounded-lg">
                                            <p className="text-sm text-blue-700 dark:text-blue-300">{uploadProgress}</p>
                                        </div>
                                    )}

                                    <div className="pt-4 border-t border-border flex gap-2">
                                        <Button
                                            onClick={handleTestPostgres}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                        >
                                            测试
                                        </Button>
                                        <Button
                                            onClick={handleUploadPostgres}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                            disabled={!!uploadProgress}
                                        >
                                            上传本地数据
                                        </Button>
                                        <Button
                                            onClick={handleSavePostgresConfig}
                                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                                        >
                                            保存配置
                                        </Button>
                                    </div>
                                </div>
                            </TabsContent>

                            {/* MySQL Configuration */}
                            <TabsContent value="mysql" className="space-y-4 mt-4">
                                <div className="space-y-4">
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor="mysql-host" className="font-semibold text-sm text-foreground">
                                                主机地址
                                            </Label>
                                            <Input
                                                id="mysql-host"
                                                type="text"
                                                placeholder="localhost 或 IP 地址"
                                                value={mysqlHost}
                                                onChange={(e) => setMysqlHost(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="mysql-port" className="font-semibold text-sm text-foreground">
                                                端口
                                            </Label>
                                            <Input
                                                id="mysql-port"
                                                type="text"
                                                placeholder="3306"
                                                value={mysqlPort}
                                                onChange={(e) => setMysqlPort(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>
                                    </div>

                                    <div className="space-y-2">
                                        <Label htmlFor="mysql-database" className="font-semibold text-sm text-foreground">
                                            数据库名称
                                        </Label>
                                        <Input
                                            id="mysql-database"
                                            type="text"
                                            placeholder="输入数据库名称"
                                            value={mysqlDatabase}
                                            onChange={(e) => setMysqlDatabase(e.target.value)}
                                            className="focus:ring-ring/20 focus:border-ring"
                                        />
                                    </div>

                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor="mysql-username" className="font-semibold text-sm text-foreground">
                                                用户名
                                            </Label>
                                            <Input
                                                id="mysql-username"
                                                type="text"
                                                placeholder="输入用户名"
                                                value={mysqlUsername}
                                                onChange={(e) => setMysqlUsername(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor="mysql-password" className="font-semibold text-sm text-foreground">
                                                密码
                                            </Label>
                                            <Input
                                                id="mysql-password"
                                                type="password"
                                                placeholder="输入密码"
                                                value={mysqlPassword}
                                                onChange={(e) => setMysqlPassword(e.target.value)}
                                                className="focus:ring-ring/20 focus:border-ring"
                                            />
                                        </div>
                                    </div>

                                    {uploadProgress && (
                                        <div className="p-3 bg-blue-50 dark:bg-blue-950 border border-blue-200 dark:border-blue-800 rounded-lg">
                                            <p className="text-sm text-blue-700 dark:text-blue-300">{uploadProgress}</p>
                                        </div>
                                    )}

                                    <div className="pt-4 border-t border-border flex gap-2">
                                        <Button
                                            onClick={handleTestMysql}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                        >
                                            测试
                                        </Button>
                                        <Button
                                            onClick={handleUploadMysql}
                                            variant="outline"
                                            className="hover:bg-muted hover:border-border"
                                            disabled={!!uploadProgress}
                                        >
                                            上传本地数据
                                        </Button>
                                        <Button
                                            onClick={handleSaveMysqlConfig}
                                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                                        >
                                            保存配置
                                        </Button>
                                    </div>
                                </div>
                            </TabsContent>
                        </Tabs>
                    </TabsContent>
                </Tabs>
            </CardContent>
        </Card>
    );
};
