/**
 * 生成 Tauri Updater 所需的 latest.json 文件
 *
 * 使用方法:
 * 1. 发布新版本到 GitHub Releases
 * 2. 运行 pnpm tauri build 构建应用
 * 3. 使用私钥签名构建产物:
 *    pnpm tauri signer sign --private-key ~/.tauri/my-app.key dist/Aipp_<version>_x64_en-US.msi.zip
 * 4. 将签名输出填入此脚本的 signature 字段
 * 5. 运行此脚本生成 latest.json
 * 6. 将生成的 latest.json 上传到 GitHub Release Assets
 */

const config = {
    version: "v0.4.0", // 修改为你的新版本号
    notes: "更新说明内容", // 修改为实际的更新说明
    pubDate: new Date().toISOString(),
    platforms: {
        "windows-x86_64": {
            signature: "YOUR_SIGNATURE_HERE", // 替换为实际签名
            url: `https://github.com/xieisabug/aipp/releases/download/v0.4.0/Aipp_0.4.0_x64_en-US.msi.zip`
        }
    }
};

const latestJson = {
    version: config.version,
    notes: config.notes,
    pub_date: config.pubDate,
    platforms: config.platforms
};

console.log(JSON.stringify(latestJson, null, 2));

// 写入文件
const fs = require('fs');
fs.writeFileSync('latest.json', JSON.stringify(latestJson, null, 2));
console.log('\nlatest.json 已生成，请将其上传到 GitHub Release Assets');
