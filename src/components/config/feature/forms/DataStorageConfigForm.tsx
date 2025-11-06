import React, { useCallback, useState } from "react";
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

    const handleSaveLocalConfig = useCallback(() => {
        toast.success("本地存储配置已保存");
    }, []);

    // Supabase handlers
    const [supabaseUrl, setSupabaseUrl] = useState("");
    const [supabaseKey, setSupabaseKey] = useState("");

    const handleSaveSupabaseConfig = useCallback(() => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请填写完整的 Supabase 配置");
            return;
        }
        // TODO: Save Supabase config to backend
        toast.info("Supabase 配置保存功能开发中");
    }, [supabaseUrl, supabaseKey]);

    const handleTestSupabase = useCallback(() => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请先填写完整的 Supabase 配置");
            return;
        }
        // TODO: Test Supabase connection
        toast.info("Supabase 连接测试功能开发中");
    }, [supabaseUrl, supabaseKey]);

    const handleUploadSupabase = useCallback(() => {
        if (!supabaseUrl || !supabaseKey) {
            toast.error("请先填写完整的 Supabase 配置");
            return;
        }
        // TODO: Upload local data to Supabase
        toast.info("上传本地数据到 Supabase 功能开发中");
    }, [supabaseUrl, supabaseKey]);

    // PostgreSQL handlers
    const [pgHost, setPgHost] = useState("");
    const [pgPort, setPgPort] = useState("5432");
    const [pgDatabase, setPgDatabase] = useState("");
    const [pgUsername, setPgUsername] = useState("");
    const [pgPassword, setPgPassword] = useState("");

    const handleSavePostgresConfig = useCallback(() => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请填写完整的 PostgreSQL 配置");
            return;
        }
        // TODO: Save PostgreSQL config to backend
        toast.info("PostgreSQL 配置保存功能开发中");
    }, [pgHost, pgDatabase, pgUsername, pgPassword]);

    const handleTestPostgres = useCallback(() => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请先填写完整的 PostgreSQL 配置");
            return;
        }
        // TODO: Test PostgreSQL connection
        toast.info("PostgreSQL 连接测试功能开发中");
    }, [pgHost, pgDatabase, pgUsername, pgPassword]);

    const handleUploadPostgres = useCallback(() => {
        if (!pgHost || !pgDatabase || !pgUsername || !pgPassword) {
            toast.error("请先填写完整的 PostgreSQL 配置");
            return;
        }
        // TODO: Upload local data to PostgreSQL
        toast.info("上传本地数据到 PostgreSQL 功能开发中");
    }, [pgHost, pgDatabase, pgUsername, pgPassword]);

    // MySQL handlers
    const [mysqlHost, setMysqlHost] = useState("");
    const [mysqlPort, setMysqlPort] = useState("3306");
    const [mysqlDatabase, setMysqlDatabase] = useState("");
    const [mysqlUsername, setMysqlUsername] = useState("");
    const [mysqlPassword, setMysqlPassword] = useState("");

    const handleSaveMysqlConfig = useCallback(() => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请填写完整的 MySQL 配置");
            return;
        }
        // TODO: Save MySQL config to backend
        toast.info("MySQL 配置保存功能开发中");
    }, [mysqlHost, mysqlDatabase, mysqlUsername, mysqlPassword]);

    const handleTestMysql = useCallback(() => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请先填写完整的 MySQL 配置");
            return;
        }
        // TODO: Test MySQL connection
        toast.info("MySQL 连接测试功能开发中");
    }, [mysqlHost, mysqlDatabase, mysqlUsername, mysqlPassword]);

    const handleUploadMysql = useCallback(() => {
        if (!mysqlHost || !mysqlDatabase || !mysqlUsername || !mysqlPassword) {
            toast.error("请先填写完整的 MySQL 配置");
            return;
        }
        // TODO: Upload local data to MySQL
        toast.info("上传本地数据到 MySQL 功能开发中");
    }, [mysqlHost, mysqlDatabase, mysqlUsername, mysqlPassword]);

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
                                    </div>

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
