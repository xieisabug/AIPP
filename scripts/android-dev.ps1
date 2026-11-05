# 自动获取本机局域网 IP 并启动 Android 开发环境
# Gradle/Javac emit UTF-8; keep native output readable in Windows PowerShell.
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
# Prefer the Android build's standard JDK 17. An explicit override remains available.
$androidJdk = $env:AIPP_ANDROID_JAVA_HOME
if (-not $androidJdk) {
    $jdkCandidates = @($env:JAVA_HOME) + @(Get-ChildItem -LiteralPath (Join-Path $env:USERPROFILE '.jdks') -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
    $androidJdk = $jdkCandidates | Where-Object {
        $_ -and (Test-Path (Join-Path $_ 'release')) -and
        (Select-String -LiteralPath (Join-Path $_ 'release') -Pattern '^JAVA_VERSION="17[.\"]' -Quiet)
    } | Select-Object -First 1
}
if ($androidJdk) {
    if (-not (Test-Path (Join-Path $androidJdk 'bin/java.exe'))) {
        throw "Invalid Android JDK directory: $androidJdk"
    }
    $env:JAVA_HOME = $androidJdk
    Write-Host "Android JAVA_HOME: $androidJdk" -ForegroundColor Cyan
}
# 获取非虚拟网卡的 IPv4 地址（排除 WSL、Docker、VPN 等虚拟网卡）

$physicalInterfaceIndexes = @(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | Select-Object -ExpandProperty ifIndex)
$networkAdapters = Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
    $_.InterfaceIndex -in $physicalInterfaceIndexes -and
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
    pnpm run tauri android dev --host $localIP @args
    exit $LASTEXITCODE
} else {
    Write-Host "Not detected IP address" -ForegroundColor Red
    Write-Host "Please set manually: `$env:TAURI_DEV_HOST = 'your IP'" -ForegroundColor Yellow
    exit 1
}
