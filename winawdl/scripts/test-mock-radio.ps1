# Offline integration only: exclusive local pipe, simulated frames, no Wintun
# or hardware. Both child processes are started hidden and always cleaned up.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$folder = Join-Path $repo 'build\awdl'
$mock = Join-Path $repo 'build\adapter\x86_64-pc-windows-gnu\debug\examples\mock_radio.exe'
$awdld = Join-Path $folder 'awdld.exe'
if (-not (Test-Path $mock) -or -not (Test-Path $awdld)) {
    throw 'Build first: .\scripts\adapter-pixi.ps1 mock; .\scripts\adapter-pixi.ps1 awdl'
}
$marker = Join-Path $folder ("mock-ready-{0}.txt" -f [guid]::NewGuid().ToString('N'))
$radio = $null
$daemon = $null
try {
    $radio = Start-Process $mock -ArgumentList "`"$marker`"" -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $folder 'mock-radio.out') -RedirectStandardError (Join-Path $folder 'mock-radio.err')
    $deadline = [datetime]::UtcNow.AddSeconds(5)
    while (-not (Test-Path $marker) -and [datetime]::UtcNow -lt $deadline) {
        $radio.Refresh()
        if ($radio.HasExited) { throw 'Mock radio exited before acquiring its exclusive pipe.' }
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path $marker)) { throw 'Mock radio did not become ready.' }
    $radio.Refresh()
    if ($radio.HasExited) { throw 'Mock radio exited before AWDL started.' }
    $daemon = Start-Process $awdld -ArgumentList '-c', '6', '-n', '-r' -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $folder 'mock-awdl.out') -RedirectStandardError (Join-Path $folder 'mock-awdl.err')
    if (-not $radio.WaitForExit(12000)) { throw 'Mock radio timed out.' }
    if ($radio.ExitCode -ne 0) { throw 'Mock radio failed; see build\awdl\mock-radio.err.' }
    if (-not $daemon.WaitForExit(5000)) { throw 'AWDL did not exit after mock disconnected.' }
    $log = Get-Content (Join-Path $folder 'mock-awdl.err') -Raw
    if ($log -notmatch 'add peer' -or $log -notmatch 'rx action [1-9]') {
        throw 'AWDL did not recognize the simulated frame; see build\awdl\mock-awdl.err.'
    }
    Write-Output 'PASS: Rust mock radio exchanged frames with AWDL; simulated peer detected.'
} finally {
    foreach ($process in @($daemon, $radio)) {
        if ($null -ne $process) { $process.Refresh(); if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force } }
    }
    Remove-Item -LiteralPath $marker -Force -ErrorAction SilentlyContinue
}
