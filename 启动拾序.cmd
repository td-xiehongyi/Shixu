@echo off
setlocal DisableDelayedExpansion
chcp 65001 >nul
set "APP_DIR=%~dp0target\x86_64-pc-windows-msvc\debug"
if not exist "%APP_DIR%\shixu-desktop.exe" (
    echo [拾序] 未找到桌面程序，请先按 README 中的 Windows 最小工作流完成构建。
    goto failed
)
if not exist "%APP_DIR%\vault-win-x64\runtime\node.exe" (
    echo [拾序] 缺少随程序构建的运行资源，请重新完成 Windows 最小工作流。
    goto failed
)
if not exist "%APP_DIR%\vault-win-x64\helper\helper.mjs" (
    echo [拾序] 缺少密码库运行文件，请重新完成 Windows 最小工作流。
    goto failed
)
if /i "%~1"=="/check" (
    echo [拾序] 程序与运行资源路径检查通过；本次未启动应用。
    exit /b 0
)
if not "%~1"=="" (
    echo [拾序] 双击即可启动；仅检查路径请使用 /check。
    goto failed
)
start "" /D "%APP_DIR%" "%APP_DIR%\shixu-desktop.exe"
if errorlevel 1 (
    echo [拾序] 无法发出启动请求，请检查程序路径与文件权限。
    goto failed
)
exit /b 0

:failed
if /i not "%~1"=="/check" pause
exit /b 1
