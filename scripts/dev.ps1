# PowerShell script to run the UI dev server and Tauri app side by side.
# For Windows users who don't have bash available.

$ErrorActionPreference = "Stop"

# Start UI dev server
$uiJob = Start-Job -ScriptBlock {
    Set-Location "ui"
    npm run dev
}

# Start Tauri app (which will spawn the core server)
$tauriJob = Start-Job -ScriptBlock {
    Set-Location "crates\tauri"
    cargo tauri dev
}

# Wait for both jobs
try {
    $uiJob | Wait-Job
    $tauriJob | Wait-Job
} finally {
    Stop-Job $uiJob -ErrorAction SilentlyContinue
    Stop-Job $tauriJob -ErrorAction SilentlyContinue
    Remove-Job $uiJob -ErrorAction SilentlyContinue
    Remove-Job $tauriJob -ErrorAction SilentlyContinue
}
