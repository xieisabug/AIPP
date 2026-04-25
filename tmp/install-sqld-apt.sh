#!/bin/sh

if [ -z "${BASH_VERSION:-}" ]; then
  if command -v bash >/dev/null 2>&1; then
    exec bash "$0" "$@"
  else
    echo "This script requires bash. Please install bash and rerun." >&2
    exit 1
  fi
fi

set -Eeuo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
APP_DIR="${APP_DIR:-/opt/aipp-sqld}"
SQLD_IMAGE="${SQLD_IMAGE:-ghcr.io/tursodatabase/libsql-server:latest}"
PUBLIC_PORT="${PUBLIC_PORT:-8080}"
ADMIN_PORT="${ADMIN_PORT:-8081}"
USE_PY_GATEWAY="${USE_PY_GATEWAY:-}"
GATEWAY_BIND="${GATEWAY_BIND:-0.0.0.0}"
GATEWAY_PORT="${GATEWAY_PORT:-9000}"
USE_NGINX="${USE_NGINX:-}"
ENABLE_UFW="${ENABLE_UFW:-}"
ENABLE_HTTPS="${ENABLE_HTTPS:-}"
DOMAIN="${DOMAIN:-}"
EMAIL="${EMAIL:-}"
TOKEN_SUBJECT="${TOKEN_SUBJECT:-aipp-client}"
TOKEN_DAYS="${TOKEN_DAYS:-3650}"
EXTRA_NAMESPACES="${EXTRA_NAMESPACES:-}"
TENANT_IDS="${TENANT_IDS:-}"
TENANT_COUNT="${TENANT_COUNT:-}"
TENANT_PATH_PREFIX="${TENANT_PATH_PREFIX:-t}"
REGENERATE_KEYS="${REGENERATE_KEYS:-}"
REGENERATE_TOKEN="${REGENERATE_TOKEN:-}"
NONINTERACTIVE="${NONINTERACTIVE:-0}"

APT_UPDATED=0
DOCKER_OS=""
CODENAME=""
HTTPS_ACTIVE=0

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  SUDO="sudo"
fi

if [ -t 0 ] && [ -t 1 ] && [ "$NONINTERACTIVE" != "1" ]; then
  INTERACTIVE=1
else
  INTERACTIVE=0
fi

if [ -t 1 ]; then
  C_RESET="$(printf '\033[0m')"
  C_BOLD="$(printf '\033[1m')"
  C_BLUE="$(printf '\033[1;34m')"
  C_GREEN="$(printf '\033[1;32m')"
  C_YELLOW="$(printf '\033[1;33m')"
  C_RED="$(printf '\033[1;31m')"
  C_CYAN="$(printf '\033[1;36m')"
else
  C_RESET=""
  C_BOLD=""
  C_BLUE=""
  C_GREEN=""
  C_YELLOW=""
  C_RED=""
  C_CYAN=""
fi

banner() {
  cat <<EOF

${C_CYAN}${C_BOLD}============================================================${C_RESET}
${C_CYAN}${C_BOLD} AIPP sqld Installer (apt / Ubuntu / Debian / Docker)${C_RESET}
${C_CYAN}${C_BOLD}============================================================${C_RESET}

这个脚本会尽量:
  - 检测已安装的软件并复用
  - 只在缺失时安装需要的组件
  - 对关键选项提供交互式选择
  - 生成 sqld 上游 token 与 tenant 凭据

EOF
}

section() {
  printf "\n${C_BLUE}${C_BOLD}==> %s${C_RESET}\n" "$*"
}

info() {
  printf "${C_BLUE}[INFO]${C_RESET} %s\n" "$*"
}

ok() {
  printf "${C_GREEN}[ OK ]${C_RESET} %s\n" "$*"
}

skip() {
  printf "${C_YELLOW}[SKIP]${C_RESET} %s\n" "$*"
}

warn() {
  printf "${C_YELLOW}[WARN]${C_RESET} %s\n" "$*"
}

die() {
  printf "${C_RED}[ERR ]${C_RESET} %s\n" "$*" >&2
  exit 1
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

package_installed() {
  dpkg -s "$1" >/dev/null 2>&1
}

validate_identifier() {
  local value="$1"
  local label="$2"

  if [[ ! "$value" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
    die "${label} 包含无效值: ${value}。只允许字母、数字、点、下划线、短横线，且必须以字母或数字开头"
  fi
}

validate_identifier_list() {
  local raw="$1"
  local label="$2"
  local item
  local normalized="${raw//,/ }"

  for item in $normalized; do
    validate_identifier "$item" "$label"
  done
}

validate_uuid() {
  local value="$1"
  local label="$2"

  if [[ ! "$value" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$ ]]; then
    die "${label} 不是合法 UUID: ${value}"
  fi
}

validate_uuid_list() {
  local raw="$1"
  local label="$2"
  local item
  local normalized="${raw//,/ }"

  for item in $normalized; do
    validate_uuid "$item" "$label"
  done
}

validate_path_prefix() {
  local value="$1"

  if [[ ! "$value" =~ ^[A-Za-z0-9_-]+$ ]]; then
    die "TENANT_PATH_PREFIX 格式无效: ${value}。只允许字母、数字、下划线和短横线"
  fi
}

validate_domain_name() {
  local value="$1"

  if [[ ! "$value" =~ ^[A-Za-z0-9.-]+$ ]]; then
    die "DOMAIN 格式无效: ${value}"
  fi
}

validate_port_value() {
  local value="$1"
  local label="$2"

  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    die "${label} 必须是数字端口，当前值: ${value}"
  fi

  if [ "$value" -lt 1 ] || [ "$value" -gt 65535 ]; then
    die "${label} 超出有效端口范围: ${value}"
  fi
}

apt_update_once() {
  if [ "$APT_UPDATED" -eq 0 ]; then
    info "执行 apt-get update"
    $SUDO apt-get update
    APT_UPDATED=1
  fi
}

ensure_packages() {
  local missing=()
  local pkg
  for pkg in "$@"; do
    if package_installed "$pkg"; then
      skip "已安装软件包: $pkg"
    else
      missing+=("$pkg")
    fi
  done

  if [ "${#missing[@]}" -gt 0 ]; then
    apt_update_once
    info "安装缺失软件包: ${missing[*]}"
    $SUDO apt-get install -y "${missing[@]}"
  fi
}

prompt_input() {
  local var_name="$1"
  local prompt_text="$2"
  local default_value="${3:-}"
  local answer=""

  if [ "${!var_name:-}" != "" ]; then
    return 0
  fi

  if [ "$INTERACTIVE" -eq 1 ]; then
    if [ -n "$default_value" ]; then
      read -r -p "$prompt_text [$default_value]: " answer
      answer="${answer:-$default_value}"
    else
      read -r -p "$prompt_text: " answer
    fi
    printf -v "$var_name" '%s' "$answer"
  else
    printf -v "$var_name" '%s' "$default_value"
  fi
}

prompt_yes_no() {
  local var_name="$1"
  local prompt_text="$2"
  local default_value="${3:-Y}"
  local default_render=""
  local answer=""

  if [ "${!var_name:-}" != "" ]; then
    case "${!var_name}" in
      1|true|TRUE|yes|YES|y|Y) printf -v "$var_name" '%s' "1" ;;
      0|false|FALSE|no|NO|n|N) printf -v "$var_name" '%s' "0" ;;
      *) die "变量 $var_name 的值无效: ${!var_name}" ;;
    esac
    return 0
  fi

  case "$default_value" in
    Y|y) default_render="Y/n" ;;
    N|n) default_render="y/N" ;;
    *) die "prompt_yes_no 默认值只能是 Y 或 N" ;;
  esac

  if [ "$INTERACTIVE" -eq 1 ]; then
    while true; do
      read -r -p "$prompt_text [$default_render]: " answer
      answer="${answer:-$default_value}"
      case "$answer" in
        Y|y|yes|YES) printf -v "$var_name" '%s' "1"; return 0 ;;
        N|n|no|NO) printf -v "$var_name" '%s' "0"; return 0 ;;
        *) warn "请输入 y 或 n" ;;
      esac
    done
  else
    case "$default_value" in
      Y|y) printf -v "$var_name" '%s' "1" ;;
      N|n) printf -v "$var_name" '%s' "0" ;;
    esac
  fi
}

docker_compose() {
  if run_docker compose version >/dev/null 2>&1; then
    run_docker compose "$@"
  elif have_cmd docker-compose; then
    if docker-compose version >/dev/null 2>&1; then
      docker-compose "$@"
    elif [ -n "$SUDO" ] && $SUDO docker-compose version >/dev/null 2>&1; then
      $SUDO docker-compose "$@"
    else
      die "检测到 docker-compose，但当前用户无权运行"
    fi
  else
    die "未找到 docker compose"
  fi
}

run_docker() {
  if docker info >/dev/null 2>&1; then
    docker "$@"
  elif [ -n "$SUDO" ] && $SUDO docker info >/dev/null 2>&1; then
    $SUDO docker "$@"
  else
    die "无法访问 Docker daemon，请检查 Docker 是否已启动，或当前用户是否具有权限"
  fi
}

detect_os() {
  section "检测系统"

  [ -f /etc/os-release ] || die "未找到 /etc/os-release，无法识别系统"
  # shellcheck disable=SC1091
  . /etc/os-release

  case "${ID:-}" in
    ubuntu) DOCKER_OS="ubuntu" ;;
    debian) DOCKER_OS="debian" ;;
    *) die "当前脚本只支持 Ubuntu / Debian，检测到 ID=${ID:-unknown}" ;;
  esac

  CODENAME="${VERSION_CODENAME:-}"
  if [ -z "$CODENAME" ] && have_cmd lsb_release; then
    CODENAME="$(lsb_release -cs)"
  fi
  [ -n "$CODENAME" ] || die "无法识别发行版代号 VERSION_CODENAME"

  ok "检测到系统: ${PRETTY_NAME:-$ID} (${CODENAME})"
}

collect_preferences() {
  section "收集部署选项"

  local default_tenant_count="1"

  prompt_input APP_DIR "部署目录" "$APP_DIR"
  prompt_input PUBLIC_PORT "sqld 对外/本机端口" "$PUBLIC_PORT"

  if [ -f "${APP_DIR}/gateway/config.json" ] && [ -z "$TENANT_COUNT" ]; then
    default_tenant_count="0"
  fi

  if [ -z "$USE_PY_GATEWAY" ]; then
    prompt_yes_no USE_PY_GATEWAY "是否部署 Python tenant gateway（推荐，支持无域名/IP、UUID tenant、每租户独立 token）" "Y"
  else
    prompt_yes_no USE_PY_GATEWAY "是否部署 Python tenant gateway（推荐，支持无域名/IP、UUID tenant、每租户独立 token）" "Y"
  fi

  if [ "$USE_PY_GATEWAY" = "1" ]; then
    prompt_input GATEWAY_BIND "Python gateway 监听地址" "$GATEWAY_BIND"
    prompt_input GATEWAY_PORT "Python gateway 对外端口" "$GATEWAY_PORT"
    prompt_input TENANT_PATH_PREFIX "tenant URL 前缀（例如 t，对应 /t/<uuid>）" "$TENANT_PATH_PREFIX"
    prompt_input TENANT_IDS "可选：预创建 tenant UUID 列表（逗号分隔）" "$TENANT_IDS"
    if [ -z "$TENANT_IDS" ]; then
      prompt_input TENANT_COUNT "本次自动生成多少个 tenant（已有配置默认 0，新部署默认 1）" "${TENANT_COUNT:-$default_tenant_count}"
    else
      TENANT_COUNT="0"
    fi
  else
    TENANT_COUNT="${TENANT_COUNT:-0}"
    TENANT_IDS=""
  fi

  if [ -z "$USE_NGINX" ]; then
    prompt_yes_no USE_NGINX "是否额外配置 Nginx/HTTPS 前置层（可选，例如给 Cloudflare 或域名入口用）" "N"
  else
    prompt_yes_no USE_NGINX "是否额外配置 Nginx/HTTPS 前置层（可选，例如给 Cloudflare 或域名入口用）" "N"
  fi

  if [ "$USE_NGINX" = "1" ]; then
    prompt_input DOMAIN "请输入域名（例如 sync.example.com）" "$DOMAIN"
    if [ -z "$ENABLE_HTTPS" ]; then
      prompt_yes_no ENABLE_HTTPS "是否尝试自动申请 HTTPS 证书" "Y"
    else
      prompt_yes_no ENABLE_HTTPS "是否尝试自动申请 HTTPS 证书" "Y"
    fi
    if [ "$ENABLE_HTTPS" = "1" ]; then
      prompt_input EMAIL "请输入 certbot 邮箱" "$EMAIL"
    fi
  else
    ENABLE_HTTPS="0"
  fi

  if [ -z "$ENABLE_UFW" ]; then
    prompt_yes_no ENABLE_UFW "是否配置 UFW 防火墙规则" "N"
  else
    prompt_yes_no ENABLE_UFW "是否配置 UFW 防火墙规则" "N"
  fi

  prompt_input TOKEN_SUBJECT "JWT token 的 subject" "$TOKEN_SUBJECT"
  prompt_input TOKEN_DAYS "JWT token 有效天数" "$TOKEN_DAYS"

  cat <<EOF

${C_BOLD}当前配置:${C_RESET}
  APP_DIR      = $APP_DIR
  PUBLIC_PORT  = $PUBLIC_PORT
  USE_PY_GATEWAY = $USE_PY_GATEWAY
  GATEWAY_BIND = ${GATEWAY_BIND:-<none>}
  GATEWAY_PORT = ${GATEWAY_PORT:-<none>}
  USE_NGINX    = $USE_NGINX
  ENABLE_HTTPS = $ENABLE_HTTPS
  ENABLE_UFW   = $ENABLE_UFW
  DOMAIN       = ${DOMAIN:-<none>}
  EMAIL        = ${EMAIL:-<none>}
  TOKEN_SUBJECT= $TOKEN_SUBJECT
  TOKEN_DAYS   = $TOKEN_DAYS
  EXTRA_NAMESPACES = ${EXTRA_NAMESPACES:-<none>}
  TENANT_PATH_PREFIX = ${TENANT_PATH_PREFIX:-<none>}
  TENANT_IDS   = ${TENANT_IDS:-<none>}
  TENANT_COUNT = ${TENANT_COUNT:-<none>}

EOF

  if [ "$USE_PY_GATEWAY" != "1" ] && [ "$USE_NGINX" != "1" ]; then
    warn "当前既未启用 Python gateway，也未启用 Nginx namespace 网关；这更适合排障，AIPP embedded sync 不建议正式直连裸 sqld"
  fi

  if [ "$INTERACTIVE" -eq 1 ]; then
    local confirmed=""
    prompt_yes_no confirmed "确认按以上配置继续" "Y"
    [ "$confirmed" = "1" ] || die "用户取消执行"
  fi
}

validate_preferences() {
  validate_port_value "$PUBLIC_PORT" "PUBLIC_PORT"
  validate_port_value "$ADMIN_PORT" "ADMIN_PORT"

  if [ "$PUBLIC_PORT" = "$ADMIN_PORT" ]; then
    die "PUBLIC_PORT 与 ADMIN_PORT 不能相同"
  fi

  if [[ ! "$TOKEN_DAYS" =~ ^[0-9]+$ ]]; then
    die "TOKEN_DAYS 必须是正整数，当前值: ${TOKEN_DAYS}"
  fi

  validate_identifier_list "$EXTRA_NAMESPACES" "EXTRA_NAMESPACES"

  if [ "$USE_PY_GATEWAY" = "1" ]; then
    validate_port_value "$GATEWAY_PORT" "GATEWAY_PORT"
    validate_path_prefix "$TENANT_PATH_PREFIX"
    validate_uuid_list "$TENANT_IDS" "TENANT_IDS"

    if [[ ! "$TENANT_COUNT" =~ ^[0-9]+$ ]]; then
      die "TENANT_COUNT 必须是非负整数，当前值: ${TENANT_COUNT}"
    fi

    if [ "$GATEWAY_PORT" = "$ADMIN_PORT" ] || [ "$GATEWAY_PORT" = "$PUBLIC_PORT" ]; then
      die "GATEWAY_PORT 不能与 PUBLIC_PORT / ADMIN_PORT 冲突"
    fi
  fi

  if [ "$USE_NGINX" = "1" ]; then
    [ -n "$DOMAIN" ] || die "启用 Nginx 时必须提供 DOMAIN"
    validate_domain_name "$DOMAIN"

    if [ "$PUBLIC_PORT" = "80" ] || [ "$PUBLIC_PORT" = "443" ]; then
      die "启用 Nginx 时 PUBLIC_PORT 不能占用 80/443；请保留给 Nginx，sqld 建议使用 8080"
    fi

    if [ "$USE_PY_GATEWAY" = "1" ] && { [ "$GATEWAY_PORT" = "80" ] || [ "$GATEWAY_PORT" = "443" ]; }; then
      die "启用 Nginx 前置层时 GATEWAY_PORT 不能占用 80/443"
    fi
  fi
}

install_base_tools() {
  section "准备基础工具"
  ensure_packages ca-certificates curl gnupg lsb-release openssl
}

maybe_cleanup_legacy_docker_packages() {
  local legacy_present=0
  local pkg
  for pkg in docker docker-engine docker.io podman-docker; do
    if package_installed "$pkg"; then
      legacy_present=1
      break
    fi
  done

  if have_cmd docker; then
    skip "检测到 docker 命令，跳过旧包清理"
    return 0
  fi

  if [ "$legacy_present" -eq 1 ]; then
    warn "检测到旧版 Docker 相关包，准备清理以避免冲突"
    $SUDO apt-get remove -y docker docker-engine docker.io containerd runc docker-compose docker-compose-v2 docker-doc podman-docker || true
  else
    skip "未检测到需要清理的旧版 Docker 包"
  fi
}

setup_docker_repo() {
  section "配置 Docker 官方仓库"

  ensure_packages ca-certificates curl gnupg lsb-release
  $SUDO install -m 0755 -d /etc/apt/keyrings

  if [ ! -f /etc/apt/keyrings/docker.asc ]; then
    info "下载 Docker GPG key"
    $SUDO curl -fsSL "https://download.docker.com/linux/${DOCKER_OS}/gpg" -o /etc/apt/keyrings/docker.asc
    $SUDO chmod a+r /etc/apt/keyrings/docker.asc
  else
    skip "Docker GPG key 已存在"
  fi

  if [ ! -f /etc/apt/sources.list.d/docker.list ] || ! grep -q "download.docker.com/linux/${DOCKER_OS}" /etc/apt/sources.list.d/docker.list 2>/dev/null; then
    info "写入 Docker apt 源"
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/${DOCKER_OS} ${CODENAME} stable" \
      | $SUDO tee /etc/apt/sources.list.d/docker.list >/dev/null
    APT_UPDATED=0
  else
    skip "Docker apt 源已存在"
  fi
}

ensure_docker() {
  section "检测 Docker"

  if have_cmd docker; then
    ok "已检测到 Docker: $(docker --version 2>/dev/null || echo present)"
  else
    warn "未检测到 Docker，开始安装"
    maybe_cleanup_legacy_docker_packages
    setup_docker_repo
    ensure_packages docker-ce docker-ce-cli containerd.io docker-buildx-plugin
  fi

  if docker compose version >/dev/null 2>&1; then
    ok "已检测到 docker compose 插件"
  elif have_cmd docker-compose; then
    ok "已检测到 docker-compose"
  else
    warn "未检测到 docker compose，开始安装插件"
    setup_docker_repo
    ensure_packages docker-compose-plugin
  fi

  info "启动并设置 Docker 开机自启"
  $SUDO systemctl enable --now docker

  if docker info >/dev/null 2>&1; then
    ok "当前用户可直接访问 Docker daemon"
  elif [ -n "$SUDO" ] && $SUDO docker info >/dev/null 2>&1; then
    warn "当前用户未直接加入 docker 组，脚本将通过 sudo 调用 Docker"
  else
    die "Docker 已安装，但当前无法访问 Docker daemon"
  fi
}

prepare_dirs() {
  section "准备目录"
  $SUDO mkdir -p "${APP_DIR}/data" "${APP_DIR}/keys" "${APP_DIR}/tools" "${APP_DIR}/gateway/credentials"
  $SUDO chown -R "$(id -un):$(id -gn)" "${APP_DIR}"
  ok "目录已就绪: ${APP_DIR}"
}

handle_existing_keys_choice() {
  if [ -f "${APP_DIR}/keys/jwt-private.pem" ] && [ -f "${APP_DIR}/keys/jwt-public.pem" ]; then
    if [ -z "$REGENERATE_KEYS" ]; then
      prompt_yes_no REGENERATE_KEYS "检测到已有 JWT 密钥，是否重新生成" "N"
    else
      prompt_yes_no REGENERATE_KEYS "检测到已有 JWT 密钥，是否重新生成" "N"
    fi
  else
    REGENERATE_KEYS="1"
  fi
}

generate_keys() {
  section "处理 JWT 密钥"
  handle_existing_keys_choice

  if [ "$REGENERATE_KEYS" = "1" ]; then
    info "生成 Ed25519 JWT 公私钥"
    openssl genpkey -algorithm Ed25519 -out "${APP_DIR}/keys/jwt-private.pem"
    openssl pkey -in "${APP_DIR}/keys/jwt-private.pem" -pubout -out "${APP_DIR}/keys/jwt-public.pem"
    chmod 600 "${APP_DIR}/keys/jwt-private.pem"
    chmod 644 "${APP_DIR}/keys/jwt-public.pem"
    ok "JWT 公私钥已生成"
  else
    skip "复用已有 JWT 公私钥"
  fi
}

ensure_python_runtime() {
  section "检测 Python 环境"

  if have_cmd python3; then
    ok "已检测到 python3: $(python3 --version 2>&1)"
  else
    warn "未检测到 python3，开始安装"
    ensure_packages python3
  fi

  if python3 -m venv --help >/dev/null 2>&1; then
    ok "已检测到 python3 venv 支持"
  else
    warn "当前 python3 缺少 venv 支持，开始安装 python3-venv"
    ensure_packages python3-venv
  fi

  if [ ! -x "${APP_DIR}/tools/venv/bin/python" ]; then
    info "创建 Python 虚拟环境"
    python3 -m venv "${APP_DIR}/tools/venv"
  else
    skip "复用已有 Python 虚拟环境"
  fi

  if "${APP_DIR}/tools/venv/bin/python" -c "import jwt, cryptography" >/dev/null 2>&1; then
    ok "JWT 所需 Python 依赖已存在"
  else
    info "安装 JWT 所需 Python 依赖"
    "${APP_DIR}/tools/venv/bin/pip" install --upgrade pip >/dev/null
    "${APP_DIR}/tools/venv/bin/pip" install pyjwt cryptography >/dev/null
  fi
}

write_token_generator() {
  section "写入 token 生成脚本"
  cat > "${APP_DIR}/tools/gen_token.py" <<'PY'
from datetime import datetime, timedelta, timezone
from pathlib import Path
import os
import jwt

private_key = Path(os.environ.get("AIPP_JWT_PRIVATE_KEY", "/opt/aipp-sqld/keys/jwt-private.pem")).read_text()
subject = os.environ.get("AIPP_TOKEN_SUBJECT", "aipp-client")
days = int(os.environ.get("AIPP_TOKEN_DAYS", "3650"))

payload = {
    "sub": subject,
    "iat": datetime.now(timezone.utc),
    "exp": datetime.now(timezone.utc) + timedelta(days=days),
}

token = jwt.encode(payload, private_key, algorithm="EdDSA")
print(token)
PY
  chmod 755 "${APP_DIR}/tools/gen_token.py"
  ok "token 生成脚本已写入"
}

handle_existing_token_choice() {
  if [ -s "${APP_DIR}/keys/aipp-token.txt" ]; then
    if [ -z "$REGENERATE_TOKEN" ]; then
      prompt_yes_no REGENERATE_TOKEN "检测到已有 sqld 上游 token，是否重新生成" "N"
    else
      prompt_yes_no REGENERATE_TOKEN "检测到已有 sqld 上游 token，是否重新生成" "N"
    fi
  else
    REGENERATE_TOKEN="1"
  fi
}

generate_persistent_token() {
  section "处理 sqld 上游访问令牌"
  handle_existing_token_choice

  if [ "$REGENERATE_TOKEN" = "1" ]; then
    info "生成 sqld 上游 token"
    AIPP_JWT_PRIVATE_KEY="${APP_DIR}/keys/jwt-private.pem" \
    AIPP_TOKEN_SUBJECT="${TOKEN_SUBJECT}" \
    AIPP_TOKEN_DAYS="${TOKEN_DAYS}" \
      "${APP_DIR}/tools/venv/bin/python" "${APP_DIR}/tools/gen_token.py" > "${APP_DIR}/keys/aipp-token.txt"
    chmod 600 "${APP_DIR}/keys/aipp-token.txt"
    ok "sqld 上游 token 已生成"
  else
    skip "复用已有 sqld 上游 token"
  fi
}

gateway_python() {
  "${APP_DIR}/tools/venv/bin/python" "$@"
}

ensure_gateway_source_files() {
  [ -f "${SCRIPT_DIR}/aipp_sqld_tenant_gateway.py" ] || die "未找到 ${SCRIPT_DIR}/aipp_sqld_tenant_gateway.py"
  [ -f "${SCRIPT_DIR}/aipp_sqld_gateway_admin.py" ] || die "未找到 ${SCRIPT_DIR}/aipp_sqld_gateway_admin.py"
}

write_gateway_tools() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  section "写入 Python tenant gateway 脚本"
  ensure_gateway_source_files

  cp "${SCRIPT_DIR}/aipp_sqld_tenant_gateway.py" "${APP_DIR}/tools/aipp_sqld_tenant_gateway.py"
  cp "${SCRIPT_DIR}/aipp_sqld_gateway_admin.py" "${APP_DIR}/tools/aipp_sqld_gateway_admin.py"
  chmod 755 "${APP_DIR}/tools/aipp_sqld_tenant_gateway.py" "${APP_DIR}/tools/aipp_sqld_gateway_admin.py"
  ok "Python tenant gateway 脚本已写入"
}

gateway_admin() {
  gateway_python "${APP_DIR}/tools/aipp_sqld_gateway_admin.py" "$@"
}

initialize_gateway_config() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  section "初始化 Python tenant gateway 配置"
  gateway_admin init \
    --config "${APP_DIR}/gateway/config.json" \
    --sqld-url "http://127.0.0.1:${PUBLIC_PORT}" \
    --sqld-token-file "${APP_DIR}/keys/aipp-token.txt" \
    --path-prefix "${TENANT_PATH_PREFIX}" >/dev/null
  ok "gateway 配置已写入"
}

provision_gateway_tenants() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  section "处理 tenant 凭据"

  local tenant
  local tenant_json=""
  local normalized_tenants="${TENANT_IDS//,/ }"
  local count="${TENANT_COUNT:-0}"
  local i

  if [ -n "$normalized_tenants" ]; then
    for tenant in $normalized_tenants; do
      gateway_admin add-tenant \
        --config "${APP_DIR}/gateway/config.json" \
        --tenant-id "${tenant}" \
        --credentials-dir "${APP_DIR}/gateway/credentials" >/dev/null
      ok "tenant 已就绪: ${tenant}"
    done
    return 0
  fi

  if [ "$count" -eq 0 ]; then
    skip "未要求新增 tenant；保留现有 gateway tenant 配置"
    return 0
  fi

  for i in $(seq 1 "$count"); do
    tenant_json="$(gateway_admin add-tenant \
      --config "${APP_DIR}/gateway/config.json" \
      --credentials-dir "${APP_DIR}/gateway/credentials")"
    tenant="$(printf '%s' "$tenant_json" | gateway_python - <<'PY'
import json
import sys

payload = json.load(sys.stdin)
print(payload["tenant_id"])
PY
)"
    ok "已生成 tenant: ${tenant}"
  done
}

list_gateway_tenants() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0
  gateway_admin list-tenants --config "${APP_DIR}/gateway/config.json" --ids-only
}

find_valid_gateway_credential() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  local tenant=""
  local credential_file=""
  local missing_count=0

  while IFS= read -r tenant; do
    [ -n "$tenant" ] || continue
    credential_file="${APP_DIR}/gateway/credentials/${tenant}.json"
    if [ -f "$credential_file" ]; then
      printf '%s\n' "$credential_file"
      return 0
    fi
    missing_count=$((missing_count + 1))
  done < <(list_gateway_tenants || true)

  if [ "$missing_count" -gt 0 ]; then
    warn "检测到已有 tenant 配置，但缺少对应凭据文件；请检查 ${APP_DIR}/gateway/credentials"
  fi

  return 1
}

write_compose() {
  local bind_spec
  local admin_bind_spec

  section "生成 Docker Compose 配置"

  if [ "$USE_NGINX" = "1" ] || [ "$USE_PY_GATEWAY" = "1" ]; then
    bind_spec="127.0.0.1:${PUBLIC_PORT}:8080"
  else
    bind_spec="${PUBLIC_PORT}:8080"
  fi
  admin_bind_spec="127.0.0.1:${ADMIN_PORT}:8081"

  cat > "${APP_DIR}/docker-compose.yml" <<EOF
services:
  sqld:
    image: ${SQLD_IMAGE}
    container_name: aipp-sqld
    restart: unless-stopped
    ports:
      - "${bind_spec}"
      - "${admin_bind_spec}"
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
EOF

  ok "docker-compose.yml 已写入"
}

create_namespace_if_needed() {
  local ns="$1"
  local status
  local body_file

  body_file="$(mktemp)"
  status="$(curl -sS -o "$body_file" -w '%{http_code}' -X POST \
    -H "Content-Type: application/json" \
    -d '{}' \
    "http://127.0.0.1:${ADMIN_PORT}/v1/namespaces/${ns}/create" || true)"

  case "$status" in
    200|201|204|409)
      ok "namespace 已就绪: ${ns}"
      ;;
    400)
      if grep -qi "already exists" "$body_file"; then
        ok "namespace 已存在: ${ns}"
      else
        warn "创建 namespace 失败: ${ns} (HTTP ${status:-curl-error})"
        cat "$body_file" >&2 || true
        rm -f "$body_file"
        die "请先确认 sqld 已启用 namespaces，且 admin 端口 ${ADMIN_PORT} 可访问"
      fi
      ;;
    *)
      warn "创建 namespace 失败: ${ns} (HTTP ${status:-curl-error})"
      cat "$body_file" >&2 || true
      rm -f "$body_file"
      die "请先确认 sqld 已启用 namespaces，且 admin 端口 ${ADMIN_PORT} 可访问"
      ;;
  esac

  rm -f "$body_file"
}

create_default_namespaces() {
  section "创建 AIPP 默认 namespaces"

  local ns
  local tenant
  local namespaces="system llm assistant mcp conversation plugin artifacts"
  local normalized_extra=""
  local configured_tenants=""

  normalized_extra="${EXTRA_NAMESPACES//,/ }"

  if [ -n "$normalized_extra" ]; then
    namespaces="${namespaces} ${normalized_extra}"
  fi

  for ns in $namespaces; do
    create_namespace_if_needed "$ns"
  done

  configured_tenants="$(list_gateway_tenants || true)"
  if [ -z "$configured_tenants" ] && [ "$USE_PY_GATEWAY" != "1" ]; then
    configured_tenants="${TENANT_IDS//,/ }"
  fi
  for tenant in $configured_tenants; do
    for ns in $namespaces; do
      create_namespace_if_needed "${tenant}-${ns}"
    done
  done
}

start_sqld() {
  section "启动 sqld"
  cd "${APP_DIR}"
  info "重建 sqld 容器，确保加载最新的密钥、数据目录挂载和配置"
  docker_compose down --remove-orphans >/dev/null 2>&1 || true
  docker_compose up -d --force-recreate
  ok "sqld 已启动"
}

wait_for_sqld_ready() {
  section "等待 sqld 就绪"

  local attempt=1
  local max_attempts=30
  local http_status=""
  local admin_status=""

  while [ "$attempt" -le "$max_attempts" ]; do
    http_status="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:${PUBLIC_PORT}/" || true)"
    admin_status="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:${ADMIN_PORT}/" || true)"

    if [ "$http_status" != "000" ] && [ "$admin_status" != "000" ]; then
      ok "sqld 已响应 (http=${http_status}, admin=${admin_status})"
      return 0
    fi

    info "sqld 尚未就绪，等待中... (${attempt}/${max_attempts})"
    sleep 1
    attempt=$((attempt + 1))
  done

  die "sqld 在预期时间内未就绪，请检查 docker logs -f aipp-sqld"
}

install_gateway_service() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  section "安装 Python tenant gateway systemd 服务"

  cat <<EOF | $SUDO tee /etc/systemd/system/aipp-sqld-gateway.service >/dev/null
[Unit]
Description=AIPP sqld tenant gateway
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
WorkingDirectory=${APP_DIR}
ExecStart=${APP_DIR}/tools/venv/bin/python ${APP_DIR}/tools/aipp_sqld_tenant_gateway.py --config ${APP_DIR}/gateway/config.json --bind ${GATEWAY_BIND} --port ${GATEWAY_PORT}
Environment=PYTHONUNBUFFERED=1
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
EOF

  $SUDO systemctl daemon-reload
  $SUDO systemctl enable aipp-sqld-gateway >/dev/null
  $SUDO systemctl restart aipp-sqld-gateway
  ok "Python tenant gateway 服务已启动"
}

wait_for_gateway_ready() {
  [ "$USE_PY_GATEWAY" = "1" ] || return 0

  section "等待 Python tenant gateway 就绪"

  local attempt=1
  local max_attempts=30
  local status=""

  while [ "$attempt" -le "$max_attempts" ]; do
    status="$(curl -sS -o /dev/null -w '%{http_code}' "http://127.0.0.1:${GATEWAY_PORT}/healthz" || true)"
    if [ "$status" = "200" ]; then
      ok "Python tenant gateway 已响应"
      return 0
    fi

    info "gateway 尚未就绪，等待中... (${attempt}/${max_attempts})"
    sleep 1
    attempt=$((attempt + 1))
  done

  die "Python tenant gateway 在预期时间内未就绪，请检查 journalctl -u aipp-sqld-gateway"
}

ensure_nginx() {
  if package_installed nginx && have_cmd nginx; then
    ok "已检测到 Nginx"
  else
    ensure_packages nginx
  fi
  $SUDO systemctl enable --now nginx
}

ensure_certbot() {
  if package_installed certbot && package_installed python3-certbot-nginx && have_cmd certbot; then
    ok "已检测到 certbot"
  else
    ensure_packages certbot python3-certbot-nginx
  fi
}

configure_nginx() {
  [ "$USE_NGINX" = "1" ] || return 0

  section "配置 Nginx"
  [ -n "$DOMAIN" ] || die "启用 Nginx 时必须提供 DOMAIN"

  ensure_nginx

  if [ "$USE_PY_GATEWAY" = "1" ]; then
    cat <<EOF | $SUDO tee /etc/nginx/sites-available/aipp-sqld.conf >/dev/null
server {
    listen 80;
    server_name ${DOMAIN};

    location / {
        proxy_pass http://127.0.0.1:${GATEWAY_PORT};
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }
}
EOF
  else
    cat <<EOF | $SUDO tee /etc/nginx/sites-available/aipp-sqld.conf >/dev/null
server {
    listen 80;
    server_name ${DOMAIN};

    location = / {
        default_type text/plain;
        return 200 "AIPP sqld namespace gateway is running\n";
    }

    location ~ ^/${TENANT_PATH_PREFIX}/(?<aipp_tenant>[^/]+)/dev/(?<aipp_db>[^/]+)(?<aipp_rest>/.*)?$ {
        set \$aipp_namespace "\$aipp_tenant-\$aipp_db";
        set \$aipp_upstream_path \$aipp_rest;
        if (\$aipp_upstream_path = "") {
            set \$aipp_upstream_path /;
        }
        proxy_pass http://127.0.0.1:${PUBLIC_PORT}\$aipp_upstream_path\$is_args\$args;
        proxy_http_version 1.1;
        proxy_set_header Host 127.0.0.1;
        proxy_set_header X-Forwarded-Host \$host;
        proxy_set_header x-namespace \$aipp_namespace;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }

    location ~ ^/dev/(?<aipp_db>[^/]+)(?<aipp_rest>/.*)?$ {
        set \$aipp_namespace "\$aipp_db";
        set \$aipp_upstream_path \$aipp_rest;
        if (\$aipp_upstream_path = "") {
            set \$aipp_upstream_path /;
        }
        proxy_pass http://127.0.0.1:${PUBLIC_PORT}\$aipp_upstream_path\$is_args\$args;
        proxy_http_version 1.1;
        proxy_set_header Host 127.0.0.1;
        proxy_set_header X-Forwarded-Host \$host;
        proxy_set_header x-namespace \$aipp_namespace;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }

    location / {
        proxy_pass http://127.0.0.1:${PUBLIC_PORT};
        proxy_http_version 1.1;
        proxy_set_header Host 127.0.0.1;
        proxy_set_header X-Forwarded-Host \$host;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }
}
EOF
  fi

  $SUDO ln -sf /etc/nginx/sites-available/aipp-sqld.conf /etc/nginx/sites-enabled/aipp-sqld.conf
  $SUDO nginx -t
  $SUDO systemctl reload nginx
  ok "Nginx 反向代理已配置"
}

configure_https() {
  [ "$USE_NGINX" = "1" ] || return 0
  [ "$ENABLE_HTTPS" = "1" ] || return 0
  [ -n "$DOMAIN" ] || die "启用 HTTPS 时必须提供 DOMAIN"
  [ -n "$EMAIL" ] || die "启用 HTTPS 时必须提供 EMAIL"

  section "配置 HTTPS"
  ensure_certbot

  if [ -d "/etc/letsencrypt/live/${DOMAIN}" ]; then
    skip "检测到已有 ${DOMAIN} 的证书，跳过重新申请"
    HTTPS_ACTIVE=1
    return 0
  fi

  info "尝试申请 Let's Encrypt 证书"
  if $SUDO certbot --nginx --non-interactive --agree-tos -m "$EMAIL" -d "$DOMAIN" --redirect; then
    HTTPS_ACTIVE=1
    ok "HTTPS 已配置"
  else
    HTTPS_ACTIVE=0
    warn "certbot 申请失败，已保留 Nginx HTTP 配置。你稍后可以在域名解析生效后重试。"
  fi
}

configure_firewall() {
  [ "$ENABLE_UFW" = "1" ] || return 0

  section "配置 UFW"

  if package_installed ufw && have_cmd ufw; then
    ok "已检测到 UFW"
  else
    ensure_packages ufw
  fi

  $SUDO ufw allow OpenSSH >/dev/null || true

  if [ "$USE_NGINX" = "1" ]; then
    $SUDO ufw allow 'Nginx Full' >/dev/null || true
    ok "已放行 Nginx Full"
  elif [ "$USE_PY_GATEWAY" = "1" ]; then
    $SUDO ufw allow "${GATEWAY_PORT}/tcp" >/dev/null || true
    ok "已放行 Python gateway 端口 ${GATEWAY_PORT}/tcp"
  else
    $SUDO ufw allow "${PUBLIC_PORT}/tcp" >/dev/null || true
    ok "已放行端口 ${PUBLIC_PORT}/tcp"
  fi

  $SUDO ufw --force enable >/dev/null || true
  ok "UFW 已启用"
}

verify_local() {
  section "本机验证"
  local token
  local credential_file=""
  local tenant_id=""
  local access_token=""
  local base_path=""
  local gateway_origin=""
  local info_status=""
  local -a gateway_values=()
  local -a curl_prefix=()
  local -a host_args=()

  token="$(cat "${APP_DIR}/keys/aipp-token.txt")"

  curl -fsS -X POST "http://127.0.0.1:${PUBLIC_PORT}/v2/pipeline" \
    -H "Authorization: Bearer ${token}" \
    -H "x-namespace: system" \
    -H "Content-Type: application/json" \
    -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}' >/dev/null

  ok "sqld 本机验证成功"

  if [ "$USE_PY_GATEWAY" != "1" ]; then
    if [ "$USE_NGINX" != "1" ]; then
      warn "未启用 Python gateway / Nginx namespace 网关：当前仅验证了裸 sqld，AIPP 客户端仍建议不要直连该地址做正式同步"
    fi
    return 0
  fi

  credential_file="$(find_valid_gateway_credential || true)"
  if [ -z "$credential_file" ]; then
    warn "未找到与当前 gateway 配置匹配的 tenant 凭据文件，跳过 gateway HTTP 验证"
    return 0
  fi

  mapfile -t gateway_values < <(gateway_python - "$credential_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)

print(payload["tenant_id"])
print(payload["access_token"])
print(payload["base_path"])
PY
)
  tenant_id="${gateway_values[0]}"
  access_token="${gateway_values[1]}"
  base_path="${gateway_values[2]}"

  gateway_origin="http://127.0.0.1:${GATEWAY_PORT}"
  curl -fsS "${gateway_origin}/healthz" >/dev/null

  info_status="$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${access_token}" \
    "${gateway_origin}${base_path}/dev/system/info" || true)"
  if [ "$info_status" = "404" ]; then
    cat <<EOF >&2
[ERROR] Python tenant gateway 的 /dev/system/info 校验返回 404。

这不是 tenant token 或 namespace 创建失败；而是当前 sqld 镜像不提供 AIPP/libsql 0.9 synced database 需要的旧同步协议端点：
  - 需要的端点: /info, /export/<generation>, /sync/...
  - 当前镜像实际可见: /v2/pipeline, /dev/<namespace>/v2/pipeline

已确认本机裸 sqld 的 /v2/pipeline 正常，但 AIPP embedded sync 依赖的 /info 协议不存在，
所以 Python gateway 转发到 sqld 根路径后会得到 404。

结论：
  - 问题不在 tenant gateway 路由本身
  - 问题在于当前 SQLD_IMAGE=${SQLD_IMAGE} 与 AIPP 使用的 libsql 0.9.30 同步协议不兼容

建议：
  1. 不要继续用 ghcr.io/tursodatabase/libsql-server:latest 做这套 AIPP embedded sync 校验
  2. 改为固定到与 libsql 0.9.30 兼容、仍提供 /info /export /sync 的 libsql-server 旧镜像标签
  3. 或者升级 AIPP 侧 libsql client / 同步实现，改用新协议

可继续人工确认：
  ACCESS_TOKEN='<tenant access_token>'
  curl -i -H "Authorization: Bearer \${ACCESS_TOKEN}" "${gateway_origin}${base_path}/dev/system/info"
  curl -i -X POST -H "Authorization: Bearer \${ACCESS_TOKEN}" -H "Content-Type: application/json" "${gateway_origin}${base_path}/dev/system/v2/pipeline" -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}'
EOF
    return 1
  elif [ "$info_status" != "200" ]; then
    die "Python tenant gateway /dev/system/info 校验失败，HTTP 状态码: ${info_status}"
  fi

  curl -fsS -X POST \
    "${gateway_origin}${base_path}/dev/system/v2/pipeline" \
    -H "Authorization: Bearer ${access_token}" \
    -H "Content-Type: application/json" \
    -d '{"baton":null,"requests":[{"type":"execute","stmt":{"sql":"select 1 as ok"}},{"type":"close"}]}' >/dev/null

  ok "Python tenant gateway 验证成功: ${tenant_id}"

  if [ "$USE_NGINX" = "1" ]; then
    if [ "$HTTPS_ACTIVE" = "1" ]; then
      curl_prefix=(curl --resolve "${DOMAIN}:443:127.0.0.1")
      host_args=()
      gateway_origin="https://${DOMAIN}"
    else
      curl_prefix=(curl)
      host_args=(-H "Host: ${DOMAIN}")
      gateway_origin="http://127.0.0.1"
    fi

    info_status="$("${curl_prefix[@]}" -sS -o /dev/null -w '%{http_code}' "${host_args[@]}" \
      -H "Authorization: Bearer ${access_token}" \
      "${gateway_origin}${base_path}/dev/system/info" || true)"
    if [ "$info_status" != "200" ]; then
      die "Nginx 前置层 /dev/system/info 校验失败，HTTP 状态码: ${info_status}"
    fi
    ok "Nginx 前置层验证成功: ${tenant_id}"
  fi
}

print_summary() {
  section "部署完成"

  local service_url
  local credential_file=""
  local -a gateway_values=()
  local tenant_id=""
  local base_path=""

  if [ "$USE_NGINX" = "1" ] && [ "$HTTPS_ACTIVE" = "1" ] && [ -n "$DOMAIN" ]; then
    service_url="https://${DOMAIN}"
  elif [ "$USE_NGINX" = "1" ] && [ -n "$DOMAIN" ]; then
    service_url="http://${DOMAIN}"
  elif [ "$USE_PY_GATEWAY" = "1" ]; then
    service_url="http://<你的服务器IP>:${GATEWAY_PORT}"
  else
    service_url="http://<你的服务器IP>:${PUBLIC_PORT}"
  fi

  cat <<EOF

${C_GREEN}${C_BOLD}------------------------------------------------------------${C_RESET}
${C_GREEN}${C_BOLD} sqld 已部署完成${C_RESET}
${C_GREEN}${C_BOLD}------------------------------------------------------------${C_RESET}

部署目录:
  ${APP_DIR}

关键文件:
  JWT 私钥   ${APP_DIR}/keys/jwt-private.pem
  JWT 公钥   ${APP_DIR}/keys/jwt-public.pem
  sqld 上游 token ${APP_DIR}/keys/aipp-token.txt

EOF

  if [ "$USE_PY_GATEWAY" = "1" ]; then
    cat <<EOF
Python gateway:
  监听地址: ${GATEWAY_BIND}:${GATEWAY_PORT}
  配置文件: ${APP_DIR}/gateway/config.json
  tenant 凭据目录: ${APP_DIR}/gateway/credentials

AIPP 里填写（每个 tenant 各一套）:
  同步服务地址: ${service_url}/${TENANT_PATH_PREFIX}/<tenant_uuid>
  访问令牌:     查看对应 tenant 凭据文件里的 access_token

说明:
  AIPP 不再直接使用 sqld 上游 token；它只交给 Python gateway 使用
  tenant UUID 会映射到 namespace 前缀，例如 <uuid>-system / <uuid>-conversation
  admin 端口保留在本机: http://127.0.0.1:${ADMIN_PORT}

常用命令:
  cd ${APP_DIR} && docker compose ps
  systemctl status aipp-sqld-gateway --no-pager
  journalctl -u aipp-sqld-gateway -f
  docker logs -f aipp-sqld
  ${APP_DIR}/tools/venv/bin/python ${APP_DIR}/tools/aipp_sqld_gateway_admin.py list-tenants --config ${APP_DIR}/gateway/config.json
  ${APP_DIR}/tools/venv/bin/python ${APP_DIR}/tools/aipp_sqld_gateway_admin.py add-tenant --config ${APP_DIR}/gateway/config.json --credentials-dir ${APP_DIR}/gateway/credentials

EOF

    while IFS= read -r tenant_id; do
      [ -n "$tenant_id" ] || continue
      credential_file="${APP_DIR}/gateway/credentials/${tenant_id}.json"
      if [ ! -f "$credential_file" ]; then
        warn "tenant ${tenant_id} 缺少凭据文件，已跳过摘要输出"
        continue
      fi
      [ -e "$credential_file" ] || continue
      mapfile -t gateway_values < <(gateway_python - "$credential_file" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    payload = json.load(fh)

print(payload["tenant_id"])
print(payload["base_path"])
print(payload["access_token"])
PY
)
      tenant_id="${gateway_values[0]}"
      base_path="${gateway_values[1]}"
      cat <<EOF
tenant:
  id: ${tenant_id}
  sync_url: ${service_url}${base_path}
  credential_file: ${credential_file}

EOF
    done < <(list_gateway_tenants || true)
  else
    local token
    token="$(cat "${APP_DIR}/keys/aipp-token.txt")"
    cat <<EOF

AIPP 里填写:
  同步服务地址: ${service_url}
  访问令牌:     ${token}

常用命令:
  cd ${APP_DIR} && docker compose ps
  cd ${APP_DIR} && docker compose restart
  docker logs -f aipp-sqld

EOF

    if [ "$USE_NGINX" = "1" ]; then
      cat <<EOF
说明:
  推荐把 AIPP 指向上面的 Nginx 网关地址，而不是裸 sqld 端口
  网关会把 /dev/<namespace>/... 改写到 sqld 根接口，并自动注入 x-namespace
  admin 端口保留在本机: http://127.0.0.1:${ADMIN_PORT}

EOF
    else
      cat <<EOF
警告:
  当前未启用 Python gateway / Nginx namespace 网关；这个 ${service_url} 更适合排障或原始 sqld 检查
  以当前 AIPP 的 embedded sync 实现，正式同步仍建议接入 Python gateway 或 Nginx/Caddy 之类的 namespace 网关
  admin 端口保留在本机: http://127.0.0.1:${ADMIN_PORT}

EOF
    fi
  fi

  if [ "$USE_PY_GATEWAY" = "0" ] && [ -n "$TENANT_IDS" ] && [ "$USE_NGINX" = "1" ]; then
    cat <<EOF
多用户 tenant 示例:
$(for tenant in ${TENANT_IDS//,/ }; do printf '  %s -> %s/%s/%s\n' "$tenant" "$service_url" "$TENANT_PATH_PREFIX" "$tenant"; done)

注意:
  /${TENANT_PATH_PREFIX}/<tenant> 这种路径能把不同用户/工作区路由到不同 namespace 前缀
  但如果多个互不信任的用户共用同一个 token，这不是完整权限隔离；正式多租户仍建议再配独立鉴权层

EOF
  fi

  if [ "$USE_NGINX" = "1" ] && [ "$ENABLE_HTTPS" != "1" ]; then
    cat <<EOF
如需启用 HTTPS，可稍后执行:
  USE_NGINX=1 ENABLE_HTTPS=1 DOMAIN=${DOMAIN} EMAIL=you@example.com bash $0

EOF
  fi
}

main() {
  banner
  have_cmd apt-get || die "未找到 apt-get"

  detect_os
  collect_preferences
  validate_preferences
  install_base_tools
  ensure_docker
  prepare_dirs
  generate_keys
  ensure_python_runtime
  write_token_generator
  generate_persistent_token
  write_gateway_tools
  write_compose
  start_sqld
  wait_for_sqld_ready
  initialize_gateway_config
  provision_gateway_tenants
  create_default_namespaces
  install_gateway_service
  wait_for_gateway_ready
  configure_nginx
  configure_https
  configure_firewall
  verify_local
  print_summary
}

main "$@"
