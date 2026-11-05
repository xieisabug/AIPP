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
# Bind all ADB calls (including Tauri's unqualified pidof/logcat) to the selected device.
$tauriArgs = @($args)
$adb = Join-Path $env:ANDROID_HOME 'platform-tools/adb.exe'
$deviceLines = @(& $adb devices -l)
if ($LASTEXITCODE -ne 0) { throw 'Unable to enumerate Android devices with adb.' }
$devices = @(foreach ($line in $deviceLines) {
    if ($line -match '^(\S+)\s+device\s+.*\bmodel:(\S+)') {
        $serial = $Matches[1]
        $model = $Matches[2]
        if ($serial -like 'emulator-*') {
            $name = @(& $adb -s $serial emu avd name)[0].Trim()
        } else {
            $name = (@(& $adb -s $serial shell getprop ro.product.model) -join '').Trim()
        }
        if ($LASTEXITCODE -ne 0 -or -not $name) { throw "Unable to read device name for $serial." }
        [PSCustomObject]@{ Serial = $serial; Name = $name; Model = $model }
    }
})
if ($devices.Count -gt 0 -and $tauriArgs -notcontains '--open' -and $tauriArgs -notcontains '-o' -and $tauriArgs -notcontains '--help' -and $tauriArgs -notcontains '-h') {
    $selectedDevice = $null
    for ($i = 0; $i -lt $tauriArgs.Count; $i++) {
        $matchesDevice = @($devices | Where-Object { $tauriArgs[$i] -in @($_.Serial, $_.Name, $_.Model) })
        if ($matchesDevice.Count -eq 1) {
            $selectedDevice = $matchesDevice[0]
            $tauriArgs = @($tauriArgs | Select-Object -First $i) + @($tauriArgs | Select-Object -Skip ($i + 1))
            break
        }
    }
    if (-not $selectedDevice -and $env:ANDROID_SERIAL) {
        $selectedDevice = $devices | Where-Object Serial -eq $env:ANDROID_SERIAL
        if (-not $selectedDevice) { throw "ANDROID_SERIAL=$env:ANDROID_SERIAL is not connected/authorized." }
    }
    if (-not $selectedDevice) {
        if ($devices.Count -eq 1) {
            $selectedDevice = $devices[0]
        } else {
            for ($i = 0; $i -lt $devices.Count; $i++) {
                Write-Host "[$($i + 1)] $($devices[$i].Name) ($($devices[$i].Serial))"
            }
            $choice = 0
            $answer = Read-Host 'Select Android device number'
            if (-not [int]::TryParse($answer, [ref]$choice) -or $choice -lt 1 -or $choice -gt $devices.Count) {
                throw 'Invalid Android device selection.'
            }
            $selectedDevice = $devices[$choice - 1]
        }
    }
    $env:ANDROID_SERIAL = $selectedDevice.Serial
    $tauriArgs = @($selectedDevice.Name) + $tauriArgs
    Write-Host "Android device: $($selectedDevice.Name), ANDROID_SERIAL=$env:ANDROID_SERIAL" -ForegroundColor Cyan
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
    pnpm run tauri android dev --host $localIP @tauriArgs
    exit $LASTEXITCODE
} else {
    Write-Host "Not detected IP address" -ForegroundColor Red
    Write-Host "Please set manually: `$env:TAURI_DEV_HOST = 'your IP'" -ForegroundColor Yellow
    exit 1
}
