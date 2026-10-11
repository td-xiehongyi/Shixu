@echo off
setlocal DisableDelayedExpansion
chcp 65001 >nul
set "APP_DIR=%~dp0target\x86_64-pc-windows-msvc\debug"
if not exist "%APP_DIR%\shixu-desktop.exe" goto missing
if not exist "%APP_DIR%\vault-win-x64\runtime\node.exe" goto missing
if not exist "%APP_DIR%\vault-win-x64\helper\helper.mjs" goto missing
if /i "%~1"=="/check" (
    echo [拾序] 虚构验收程序与资源路径检查通过；本次未启动应用。
    exit /b 0
)
if not "%~1"=="" (
    echo [拾序] 双击启动虚构密码库验收；仅检查路径请使用 /check。
    goto failed
)
echo [拾序] 仅使用虚构数据。专用密码库会保留在程序旁 shixu-fictional-desktop 目录。
start "" /D "%APP_DIR%" "%APP_DIR%\shixu-desktop.exe" --vault-fictional
if errorlevel 1 goto failed
exit /b 0
:missing
echo [拾序] 缺少桌面程序或运行资源，请按 README 的 Windows 最小工作流完成构建。
:failed
if /i not "%~1"=="/check" pause
exit /b 1
