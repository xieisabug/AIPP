# AIPP 多端同步服务端部署文档（apt-get / Ubuntu / Debian / Docker 版）

## 一、文档目标

这份文档面向 **Ubuntu / Debian** 服务器，使用 `apt-get` 安装 Docker，并通过 Docker 部署 `sqld`（`ghcr.io/tursodatabase/libsql-server`）来给 AIPP 提供多端同步后端。

文档覆盖以下内容：

- 从零安装 Docker 和 `docker compose`
- 生成 `sqld` 所需的 JWT 公私钥
- 启动 `sqld` 主库服务
- 部署 Python tenant gateway（支持无域名/IP、UUID tenant、每租户独立 token）
- 生成 AIPP 客户端真正要填写的 tenant 访问令牌
- 验证 `sqld` 与 gateway 是否可用
- 可选配置：Nginx / Cloudflare / HTTPS / UFW
- 日常运维、升级、备份和排错

---

## 二、先说明三个关键事实

### 1. AIPP 客户端现在需要的服务端信息只有两项

在 AIPP 的同步设置里，当前核心要填的是：

- `同步服务地址`
- `访问令牌`

其中：

- `同步服务地址` 指向你的同步网关地址，例如 `http://服务器IP:9000/t/<uuid>` 或 `https://sync.example.com/t/<uuid>`
- `访问令牌` 指的是 **tenant 自己的 gateway token**，不是 `sqld` 上游 JWT

### 2. `sqld` 只负责“远端主库 + 同步协议”

这份部署文档能解决的是：

- 把 libSQL/sqld 服务端跑起来
- 让 AIPP 客户端通过本地优先的 synced database + 远端 sqld 进行同步
- 为 AIPP 的每个数据库提供独立 namespace
- 用 Python tenant gateway 把 AIPP 自动访问的 `/dev/<namespace>` 转成 sqld 根接口，并自动补上 `x-namespace`
- 在同一个 sqld 实例前继续扩展出多用户/多 tenant 路由前缀
- 给每个 tenant 生成单独的访问 token，而不是让所有人共用同一个 sqld JWT

当前 AIPP 客户端已经能在这套 `sqld + namespace 网关` 方案里真实执行：

- `UseLocal`
- `BackupThenUseRemote`
- `UseRemote`

但 `AppendLocal` 这种“本地和云端自动合并 + 去重 + ID 重映射”仍然不是 plain `sqld` 自动提供的业务能力。

这意味着：

- `UseRemote / BackupThenUseRemote`：AIPP 客户端自己先备份 / 清空本地，再从远端 namespace 拉取
- `UseLocal`：AIPP 客户端自己清空远端 namespace，再把本地 SQLite 内容导入并推送
- `AppendLocal`：**当前仍不支持**

### 3. 本文优先给出“能稳定跑通”的单机部署

这份文档默认：

- 单台 Ubuntu / Debian 机器
- 一个 `sqld` 主实例
- 一套 JWT 公钥验证
- 已开启 `namespaces`
- 已开放 `admin` 端口供你预创建 namespace
- AIPP 客户端连接前面的 Python tenant gateway，而不是直接裸连 sqld

后续如果你要做：

- 多租户
- 多 namespace
- 官方云同步服务
- 一键登录后自动下发 token

那是在这份部署之上继续扩展。

### 4. Cloudflare 的 HTTPS 能解决到什么程度

如果你把 Python gateway 暴露给 Cloudflare：

- **客户端 -> Cloudflare** 这段链路可以是 HTTPS
- **Cloudflare -> 你的源站** 这段链路是否加密，取决于你用的是哪种模式

如果只是 Cloudflare 的 **Flexible**，那源站仍然可能是明文 HTTP。  
如果你使用 **Full / Full (strict)** 并且源站自己也有 HTTPS（例如 Nginx + certbot / origin cert / tunnel），那才是端到端都加密。

所以：

- 如果你接受源站 HTTP，并且已经有 IP 白名单 / 内网 / Tunnel 等额外保护，这份文档里的 Python gateway 模式可以直接用
- 如果你要求 Cloudflare 到源站这段也加密，那就在 Python gateway 前面再加 Nginx/HTTPS

---

## 三、部署前准备

建议先确认系统和架构：

```bash
cat /etc/os-release
uname -m
```

推荐环境：

- Ubuntu 22.04 / 24.04
- Debian 12 / 13
- `x86_64` 或 `aarch64`

还建议确认时钟正确，否则 JWT 容易因为时间偏差导致鉴权失败：

```bash
timedatectl
```

如果时间不对，先修正 NTP。

---

## 四、安装 Docker（apt-get 官方仓库方式）

### 1. 卸载旧版 Docker 相关包

```bash
sudo apt-get remove -y docker docker-engine docker.io containerd runc docker-compose docker-compose-v2 docker-doc podman-docker || true
```

### 2. 安装基础依赖

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg lsb-release
```

### 3. 添加 Docker 官方 GPG key

```bash
sudo install -m 0755 -d /etc/apt/keyrings
```

Ubuntu：

```bash
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
```

Debian：

```bash
sudo curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
```

### 4. 添加 Docker 官方 apt 仓库

如果是 Ubuntu：

```bash
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | \
  sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
```

如果是 Debian：

```bash
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | \
  sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
```

### 5. 安装 Docker Engine 和 Compose 插件

```bash
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
```

### 6. 启动 Docker

```bash
sudo systemctl enable --now docker
sudo systemctl status docker --no-pager
```

### 7. 验证 Docker 是否安装成功

```bash
sudo docker --version
sudo docker compose version
sudo docker run --rm hello-world
```

### 8. 可选：让当前用户直接运行 Docker

```bash
sudo usermod -aG docker $USER
newgrp docker
docker run --rm hello-world
```

---

## 五、准备 AIPP 同步服务目录

建议统一放到 `/opt/aipp-sqld`：

```bash
sudo mkdir -p /opt/aipp-sqld/{data,keys,tools}
sudo chown -R $USER:$USER /opt/aipp-sqld
cd /opt/aipp-sqld
```

目录用途：

- `data/`：`sqld` 的数据库数据目录
- `keys/`：JWT 公私钥
- `tools/`：生成 token 的辅助脚本和虚拟环境

---

## 六、生成 JWT 公私钥

`sqld` 推荐使用 **Ed25519 + JWT** 验证客户端请求。

生成私钥和公钥：

```bash
openssl genpkey -algorithm Ed25519 -out /opt/aipp-sqld/keys/jwt-private.pem
openssl pkey -in /opt/aipp-sqld/keys/jwt-private.pem -pubout -out /opt/aipp-sqld/keys/jwt-public.pem
```

生成后会得到两个文件：

- `/opt/aipp-sqld/keys/jwt-private.pem`
- `/opt/aipp-sqld/keys/jwt-public.pem`

### 重要安全说明

- `jwt-private.pem` 只能保留在服务器上
- `jwt-public.pem` 提供给 `sqld` 用于验签
- AIPP 客户端里填写的不是公钥；真正给客户端的是后面 tenant 凭据文件里的 `access_token`

建议设置私钥权限：

```bash
chmod 600 /opt/aipp-sqld/keys/jwt-private.pem
chmod 644 /opt/aipp-sqld/keys/jwt-public.pem
```

---

## 七、编写 docker-compose.yml

在 `/opt/aipp-sqld/docker-compose.yml` 中写入：

```yaml
services:
  sqld:
    image: ghcr.io/tursodatabase/libsql-server:latest
    container_name: aipp-sqld
    restart: unless-stopped
    ports:
      - "127.0.0.1:8080:8080"
      - "127.0.0.1:8081:8081"
    environment:
      - SQLD_NODE=standalone
      - SQLD_DB_PATH=/var/lib/sqld
      - SQLD_HTTP_LISTEN_ADDR=0.0.0.0:8080
    command:
      - /bin/sqld
      - --admin-listen-addr
      - 0.0.0.0:8081
      - --enable-namespaces
      - --auth-jwt-key-file
      - /keys/jwt-public.pem
    volumes:
      - ./data:/var/lib/sqld
      - ./keys/jwt-public.pem:/keys/jwt-public.pem:ro
```

### 说明

这份配置里：

- `8080` 是客户端 HTTP / 同步端口
- `8081` 是 namespace 管理用的 admin 端口
- 两个端口都只监听在宿主机本机回环地址
- 外网默认不能直接访问
- `SQLD_DB_PATH=/var/lib/sqld` 很重要：上游镜像会在降权到 `sqld` 用户前先对这个路径做 `chown`

这是为了后续更推荐地通过 **Python tenant gateway**（以及可选的 Nginx / Cloudflare）暴露公网入口。

> `sqld` 本身不再建议直接给 AIPP 当公网入口。  
> AIPP 应该连接 Python gateway，例如 `http://服务器IP:9000/t/<tenant_uuid>`；  
> 客户端会自动按数据库名继续访问 `/dev/system`、`/dev/conversation`、`/dev/artifacts` 等路径，  
> gateway 再把这些路径改写成 sqld 根接口，并自动补上 `x-namespace: <tenant_uuid>-system` 这类头。

如果你只是临时测试，不上 Nginx，也可以把端口映射改成：

```yaml
ports:
  - "8080:8080"
  - "8081:8081"
```

这样外网可以直接访问 `http://你的服务器IP:8080`，但这更适合原始 sqld 排障；**当前 AIPP 的 embedded sync 不建议直连这个裸地址**。

---

## 八、启动 sqld

```bash
cd /opt/aipp-sqld
docker compose up -d
docker ps
docker logs -f aipp-sqld
```

常见成功状态：

- `docker ps` 能看到 `aipp-sqld`
- `docker logs -f aipp-sqld` 没有持续报错

### 额外一步：创建 AIPP 默认 namespace

AIPP 至少会用到这些 namespace：

- `system`
- `llm`
- `assistant`
- `mcp`
- `conversation`
- `plugin`
- `artifacts`

可以用 admin API 依次创建：

```bash
for ns in system llm assistant mcp conversation plugin artifacts; do
  curl -fsS -X POST "http://127.0.0.1:8081/v1/namespaces/${ns}/create" \
    -H "Content-Type: application/json" \
    -d '{}'
done
```

不同 libsql / sqld 版本在 namespace 已存在时可能返回 `409`，也可能返回 `400` + `already exists`；这两种都可以按“已就绪”处理。

如果你已经有历史 artifact 动态数据库，后续还要按实际文件名再额外创建类似 `artifact-data-xxx` 的 namespace。

`tmp/install-sqld-apt.sh` 现在会在建 namespace 前等待 sqld/public/admin 端口就绪；如果你想在安装时顺手创建额外 namespace，可以这样执行：

```bash
EXTRA_NAMESPACES="artifact-data-foo,artifact-data-bar" bash tmp/install-sqld-apt.sh
```

如果你还想顺手为多个 tenant 预创建隔离 namespace，可以这样执行：

```bash
TENANT_IDS="5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3,9d402ec6-c8cf-45a6-bcb0-7f122874fc9a" bash tmp/install-sqld-apt.sh
```

这样会额外创建类似：

- `5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3-system`
- `9d402ec6-c8cf-45a6-bcb0-7f122874fc9a-conversation`

如果容器没起来，先执行：

```bash
docker compose ps
docker logs --tail 200 aipp-sqld
```

---

## 九、安装 Python 工具并生成 sqld 上游 token

Python 这里主要做两件事：

- 生成 **sqld 上游 JWT token**（只给 gateway 用，不给 AIPP 客户端）
- 运行 **tenant gateway / tenant 管理脚本**

### 1. 安装 Python 和 venv

```bash
sudo apt-get update
sudo apt-get install -y python3 python3-venv
```

### 2. 创建虚拟环境

```bash
python3 -m venv /opt/aipp-sqld/tools/venv
/opt/aipp-sqld/tools/venv/bin/pip install --upgrade pip
/opt/aipp-sqld/tools/venv/bin/pip install pyjwt cryptography
```

### 3. 创建 sqld 上游 token 生成脚本

在 `/opt/aipp-sqld/tools/gen_token.py` 中写入：

```python
from datetime import datetime, timedelta, timezone
from pathlib import Path
import jwt

private_key = Path("/opt/aipp-sqld/keys/jwt-private.pem").read_text()

payload = {
    "sub": "aipp-gateway",
    "iat": datetime.now(timezone.utc),
    "exp": datetime.now(timezone.utc) + timedelta(days=3650),
}

token = jwt.encode(payload, private_key, algorithm="EdDSA")
print(token)
```

### 4. 生成 sqld 上游 token

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/gen_token.py > /opt/aipp-sqld/keys/aipp-token.txt
chmod 600 /opt/aipp-sqld/keys/aipp-token.txt
```

注意：这个 `aipp-token.txt` 是 **gateway 访问 sqld 的内部 token**，不要直接发给 AIPP 客户端。

---

## 十、部署 Python tenant gateway

把下面两个文件和 `tmp/install-sqld-apt.sh` 一起放到服务器上：

- `aipp_sqld_tenant_gateway.py`
- `aipp_sqld_gateway_admin.py`

脚本会自动把它们复制到 `/opt/aipp-sqld/tools/`。

### 0. 最省事的一键执行方式

如果你就是想走 **无域名 + 直接暴露 Python gateway + UUID tenant**，最简单的是：

```bash
USE_PY_GATEWAY=1 USE_NGINX=0 GATEWAY_PORT=9000 bash tmp/install-sqld-apt.sh
```

如果你想在安装时直接预置两个 UUID tenant：

```bash
USE_PY_GATEWAY=1 USE_NGINX=0 GATEWAY_PORT=9000 \
TENANT_IDS="5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3,9d402ec6-c8cf-45a6-bcb0-7f122874fc9a" \
bash tmp/install-sqld-apt.sh
```

如果你以后想把 Cloudflare / 域名 / HTTPS 再接进来，也不影响这套结构。

### 1. 初始化 gateway 配置

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/aipp_sqld_gateway_admin.py init \
  --config /opt/aipp-sqld/gateway/config.json \
  --sqld-url http://127.0.0.1:8080 \
  --sqld-token-file /opt/aipp-sqld/keys/aipp-token.txt \
  --path-prefix t
```

### 2. 创建 tenant（默认建议用 UUID）

自动生成一个 tenant：

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/aipp_sqld_gateway_admin.py add-tenant \
  --config /opt/aipp-sqld/gateway/config.json \
  --credentials-dir /opt/aipp-sqld/gateway/credentials
```

如果你想自己指定 tenant UUID：

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/aipp_sqld_gateway_admin.py add-tenant \
  --config /opt/aipp-sqld/gateway/config.json \
  --tenant-id 5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3 \
  --credentials-dir /opt/aipp-sqld/gateway/credentials
```

这会生成：

- gateway 配置中的 tenant 记录
- `/opt/aipp-sqld/gateway/credentials/<tenant_id>.json`

凭据文件里会有：

- `tenant_id`
- `base_path`
- `access_token`

### 3. 创建 systemd 服务

```ini
[Unit]
Description=AIPP sqld tenant gateway
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
WorkingDirectory=/opt/aipp-sqld
ExecStart=/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/aipp_sqld_tenant_gateway.py --config /opt/aipp-sqld/gateway/config.json --bind 0.0.0.0 --port 9000
Environment=PYTHONUNBUFFERED=1
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
```

启动：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now aipp-sqld-gateway
sudo systemctl status aipp-sqld-gateway --no-pager
```

推荐入口结构：

```text
AIPP Client
    ↓ HTTP / HTTPS
Python tenant gateway :9000
    ↓ localhost + internal sqld token + x-namespace
sqld :8080
```

---

## 十一、先在本机验证服务

### 1. 先验证裸 sqld 是否正常

```bash
SQ_TOKEN=$(cat /opt/aipp-sqld/keys/aipp-token.txt)

curl -fsS -X POST http://127.0.0.1:8080/v2/pipeline \
  -H "Authorization: Bearer $SQ_TOKEN" \
  -H "x-namespace: system" \
  -H "Content-Type: application/json" \
  -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}'
```

这一步验证的是：

- `sqld` 本体是否正常
- 内部 JWT 是否有效
- `system` namespace 是否已创建

### 2. 再验证 Python tenant gateway 是否正常

找一个 tenant 凭据文件：

```bash
cat /opt/aipp-sqld/gateway/credentials/*.json
```

假设其中有：

- `tenant_id = 5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3`
- `access_token = ...`
- `base_path = /t/5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3`

就可以测试：

```bash
GW_TOKEN="这里填 access_token"
BASE_PATH="/t/5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3"

curl -fsS http://127.0.0.1:9000/healthz

curl -fsS "http://127.0.0.1:9000${BASE_PATH}/dev/system/info" \
  -H "Authorization: Bearer $GW_TOKEN" \
  >/dev/null

curl -fsS -X POST "http://127.0.0.1:9000${BASE_PATH}/dev/system/v2/pipeline" \
  -H "Authorization: Bearer $GW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}'
```

这一步验证的是：

- gateway 自己是否启动
- tenant token 是否正确
- gateway 是否已把 `/t/<uuid>/dev/system/...` 转成 sqld 根接口
- gateway 是否已自动注入 `x-namespace: <uuid>-system`

如果失败，先看日志：

```bash
docker logs --tail 200 aipp-sqld
journalctl -u aipp-sqld-gateway -f
```

---

## 十二、可选前置层：Nginx / Cloudflare

如果你已经有 Cloudflare、反向代理或 IP 白名单体系，Python gateway 完全可以直接跑在：

```text
http://服务器IP:9000
```

如果你还想再加一层 Nginx 做域名或 HTTPS，可以让 Nginx **直接反代 Python gateway**，而不是再自己做 namespace 重写：

```nginx
server {
    listen 80;
    server_name sync.example.com;

    location / {
        proxy_pass http://127.0.0.1:9000;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

如果你要 Let’s Encrypt：

```bash
sudo apt-get install -y nginx certbot python3-certbot-nginx
sudo systemctl enable --now nginx
sudo certbot --nginx -d sync.example.com
```

这样 AIPP 里就可以填：

```text
https://sync.example.com/t/<tenant_uuid>
```

如果你用 Cloudflare：

- 只做 CDN/代理、源站仍是 HTTP：那 Python gateway 可以继续保留 HTTP
- 如果你要求 Cloudflare 到源站也必须加密：就在 gateway 前再加 Nginx + HTTPS，或改用 Tunnel / origin cert

### UFW 建议

如果你直接暴露 Python gateway：

```bash
sudo apt-get install -y ufw
sudo ufw allow OpenSSH
sudo ufw allow 9000/tcp
sudo ufw enable
sudo ufw status
```

如果你走 Nginx + HTTPS：

```bash
sudo ufw allow 'Nginx Full'
```

---

## 十三、AIPP 客户端填写方式

在 AIPP 的同步设置页中：

- `启用多端同步`：开启
- `同步服务地址`：填写 tenant 对应的 gateway 地址
- `访问令牌`：填写 tenant 凭据文件里的 `access_token`
- `同步方式`：建议先选 `手动同步`

典型填写方式：

### 如果直接暴露 Python gateway

- `同步服务地址`：`http://服务器IP:9000/t/5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3`
- `访问令牌`：`/opt/aipp-sqld/gateway/credentials/5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3.json` 中的 `access_token`

### 如果前面有 Nginx / Cloudflare 域名

- `同步服务地址`：`https://sync.example.com/t/5b97f0e7-3cc5-4df4-b8a5-4bca8b7ff2d3`
- `访问令牌`：对应 tenant 的 `access_token`

### 以后要新增 tenant

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/aipp_sqld_gateway_admin.py add-tenant \
  --config /opt/aipp-sqld/gateway/config.json \
  --credentials-dir /opt/aipp-sqld/gateway/credentials
```

然后再为这个新 UUID 创建对应 namespace（或直接重新执行安装脚本，它会自动补默认 namespace）：

- `<uuid>-system`
- `<uuid>-llm`
- `<uuid>-assistant`
- `<uuid>-mcp`
- `<uuid>-conversation`
- `<uuid>-plugin`
- `<uuid>-artifacts`

---

## 十四、升级与日常运维

### 查看容器状态

```bash
cd /opt/aipp-sqld
docker compose ps
```

### 查看日志

```bash
docker logs -f aipp-sqld
```

### 重启服务

```bash
cd /opt/aipp-sqld
docker compose restart
```

### 停止服务

```bash
cd /opt/aipp-sqld
docker compose down
```

### 拉取最新镜像并升级

```bash
cd /opt/aipp-sqld
docker compose pull
docker compose up -d
```

### 备份数据目录

```bash
sudo tar czf /opt/aipp-sqld-backup-$(date +%F-%H%M%S).tar.gz /opt/aipp-sqld/data /opt/aipp-sqld/keys
```

---

## 十五、常见问题排查

### 1. `docker compose up -d` 失败

先看 Docker 服务是否正常：

```bash
sudo systemctl status docker --no-pager
journalctl -u docker -n 100 --no-pager
```

再看容器日志：

```bash
docker compose ps
docker logs --tail 200 aipp-sqld
```

### 2. `curl` 鉴权失败

常见原因：

- 你把公钥当成了访问令牌
- 私钥和公钥不是同一对
- 服务器时间不正确，导致 JWT `exp` / `iat` 校验失败
- AIPP 里 token 粘贴不完整

建议检查：

```bash
timedatectl
head -n 5 /opt/aipp-sqld/keys/jwt-public.pem
head -n 5 /opt/aipp-sqld/keys/jwt-private.pem
```

重新生成 token：

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/gen_token.py
```

### 3. AIPP 连不上

优先检查：

- 地址是不是填成了 `https` / `http` 错误版本
- 防火墙是不是没放行
- 如果经过 Nginx，域名证书是否正常
- `docker logs -f aipp-sqld` 是否有请求进入

### 4. 端口冲突

如果 8080 被占用：

```bash
sudo ss -ltnp | grep 8080
```

可以把 `docker-compose.yml` 里的对外端口改掉，例如：

```yaml
ports:
  - "127.0.0.1:18080:8080"
```

然后 Nginx 反代到 `127.0.0.1:18080`。

---

## 十六、这份部署当前能做到什么，不能做到什么

### 现在能做到

- 在 Ubuntu / Debian 服务器上用 Docker 跑起 `sqld`
- 用 JWT 验证 AIPP 客户端访问
- 让 AIPP 对接一个自建 libSQL 同步服务
- 支撑 AIPP 的 `UseRemote`、`UseLocal`、`BackupThenUseRemote`

### 现在还不能自动做到

- 多台历史旧 SQLite 自动无冲突合并
- `AppendLocal` 的真正自动合并、去重和 ID 重映射
- 用户点一下“登录”就自动获得 token 的官方云服务交互
- 多租户 SaaS 管理后台

这些都需要后续再加：

- Auth Gateway
- 用户系统
- 更完整的 namespace 生命周期管理
- 高级冲突合并控制服务

---

## 十七、建议的最小上线顺序

建议你按这个顺序推进：

1. 先按本文把 `sqld` 在 Ubuntu / Debian 上跑起来
2. AIPP 里先用 `手动同步 + 固定 JWT` 跑通第一条链路
3. 再加 `Nginx + HTTPS`
4. 最后再做：
   - 官方云服务
   - 自动签发 token
   - `AppendLocal` 的服务端控制逻辑

---

## 十八、快速命令清单

### 安装 Docker

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg lsb-release
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl enable --now docker
```

> 如果是 Debian，把上面命令里的 `linux/ubuntu` 换成 `linux/debian`。

### 生成密钥

```bash
openssl genpkey -algorithm Ed25519 -out /opt/aipp-sqld/keys/jwt-private.pem
openssl pkey -in /opt/aipp-sqld/keys/jwt-private.pem -pubout -out /opt/aipp-sqld/keys/jwt-public.pem
```

### 启动 sqld

```bash
cd /opt/aipp-sqld
docker compose up -d
docker logs -f aipp-sqld
```

### 生成 AIPP 访问令牌

```bash
/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/gen_token.py
```

### 测试 sqld

```bash
TOKEN=$(/opt/aipp-sqld/tools/venv/bin/python /opt/aipp-sqld/tools/gen_token.py)
curl -X POST http://127.0.0.1:8080/v2/pipeline \
  -H "Authorization: Bearer $TOKEN" \
  -H "x-namespace: system" \
  -H "Content-Type: application/json" \
  -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}'
```
