# 自动获取本机局域网 IP 并启动 Android 开发环境
# 获取非虚拟网卡的 IPv4 地址（排除 WSL、Docker、VPN 等虚拟网卡）

$networkAdapters = Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
    $_.IPAddress -notlike '127.*' -and 
    $_.IPAddress -notlike '169.254.*' -and
    $_.InterfaceAlias -notmatch 'vEthernet|WSL|Docker|VPN|Loopback'
}

# 优先选择以太网或 Wi-Fi 适配器
$preferredAdapter = $networkAdapters | Where-Object {
    $_.InterfaceAlias -match 'Ethernet|Wi-Fi|WLAN|以太网|无线'
} | Select-Object -First 1

if (-not $preferredAdapter) {
    $preferredAdapter = $networkAdapters | Select-Object -First 1
}

if ($preferredAdapter) {
    $localIP = $preferredAdapter.IPAddress
    Write-Host "Detect IP: $localIP" -ForegroundColor Green
    Write-Host "Network Adapter: $($preferredAdapter.InterfaceAlias)" -ForegroundColor Cyan
    
    # 设置环境变量
    $env:TAURI_DEV_HOST = $localIP
    
    Write-Host ""
    Write-Host "Run Android development environment..." -ForegroundColor Yellow
    Write-Host ""
    
    # 启动 Tauri Android 开发
    pnpm run tauri android dev
} else {
    Write-Host "Not detected IP address" -ForegroundColor Red
    Write-Host "Please set manually: `$env:TAURI_DEV_HOST = 'your IP'" -ForegroundColor Yellow
    exit 1
}
