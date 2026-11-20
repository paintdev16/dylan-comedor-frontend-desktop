param(
    [string]$MakeNsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    [switch]$TestStable
)

$ErrorActionPreference = 'Stop'

$MakeNsis = (Resolve-Path -LiteralPath $MakeNsis).Path
$installerDir = $PSScriptRoot
$setupNsi = Join-Path $installerDir 'setup.nsi'
$veltrixRelease = Get-Content -Raw (Join-Path $installerDir 'veltrix-release.json') | ConvertFrom-Json
$veltrixFilename = "veltrix-$($veltrixRelease.tag).exe"

$workRoot = Join-Path ([IO.Path]::GetTempPath()) ("dylan-nsis-e2e-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $workRoot | Out-Null

function Build-TestSetup {
    param(
        [string]$Version,
        [string]$ExePayload,
        [switch]$Stable
    )
    $buildDir = Join-Path $workRoot ("build-" + $Version + $(if ($Stable) { '-stable' } else { '-beta' }))
    $inputs = Join-Path $buildDir 'inputs'
    New-Item -ItemType Directory -Force $inputs | Out-Null

    Set-Content -LiteralPath (Join-Path $inputs 'dylan.exe') -Value $ExePayload -NoNewline -Encoding ascii
    Set-Content -LiteralPath (Join-Path $inputs 'veltrix.exe') -Value "MZ-veltrix-$($veltrixRelease.tag)" -NoNewline -Encoding ascii
    $cfgName = if ($Stable) { 'app-stable.json' } else { 'app.json' }
    Copy-Item -LiteralPath (Join-Path $installerDir $cfgName) -Destination (Join-Path $inputs 'app.json') -Force
    Set-Content -LiteralPath (Join-Path $inputs 'version.txt') -Value $Version -Encoding ascii

    $args = @(
        '/V2',
        "/DAPP_VERSION=$Version",
        "/DPILOT_INPUT=$inputs",
        "/DOUT_DIR=$buildDir",
        "/DVELTRIX_FILENAME=$veltrixFilename"
    )
    if ($Stable) { $args += '/DAPP_STABLE=1' }
    $args += $setupNsi

    & $MakeNsis @args
    if ($LASTEXITCODE) { throw "Falló MakeNSIS para $Version" }
    return (Join-Path $buildDir "Dylan-Setup-$Version-x64.exe")
}

function Run-Suite {
    param([switch]$Stable)

    $modeLabel = if ($Stable) { 'STABLE (Dylan + Uninstall.exe)' } else { 'BETA (Dylan-Test-Veltrix)' }
    $appDirName = if ($Stable) { 'Dylan' } else { 'Dylan-Test-Veltrix' }
    $installDir = Join-Path $env:LOCALAPPDATA "Programs\$appDirName"
    $versions = if ($Stable) {
        @('0.1.3', '0.1.4', '0.1.5', '0.1.6', '0.1.7')
    } else {
        @('1.0.0-beta.1', '1.0.0-beta.2', '1.0.0-beta.3', '1.0.0-beta.4', '1.0.0-beta.5')
    }

    if (Test-Path -LiteralPath $installDir) {
        Remove-Item -LiteralPath $installDir -Recurse -Force
    }

    Write-Output "=== Suite NSIS: $modeLabel ==="

    try {
        # 1. Rechazo de ruta destino no permitida (seguridad)
        $setup0 = Build-TestSetup -Version $versions[0] -ExePayload "MZ-dylan-$($versions[0])" -Stable:$Stable
        $foreignDir = Join-Path $workRoot 'destino no permitido'
        $r0 = Start-Process -FilePath $setup0 -ArgumentList @('/S', "/D=$foreignDir") -Wait -PassThru -WindowStyle Hidden
        if ($r0.ExitCode -ne 20 -or (Test-Path -LiteralPath $foreignDir)) {
            throw "NSIS no rechazó destino ajeno (código $($r0.ExitCode))"
        }
        Write-Output "OK [1/4]: Rechazo de ruta destino ajena (exit=20)."

        # 2. Instalación inicial + datos del usuario + actualizaciones repetidas con Veltrix bloqueado
        $rInit = Start-Process -FilePath $setup0 -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
        if ($rInit.ExitCode -ne 0) {
            throw "Instalación inicial falló (código $($rInit.ExitCode))"
        }
        New-Item -ItemType Directory -Force (Join-Path $installDir 'data') | Out-Null
        $userFile = Join-Path $installDir 'data\comedor.db'
        Set-Content -LiteralPath $userFile -Value 'datos-criticos-comedor' -NoNewline
        $userHash = (Get-FileHash -LiteralPath $userFile -Algorithm SHA256).Hash

        $lockUpdate = [IO.File]::Open((Join-Path $installDir '.updater\update.lock'), 'Open', 'ReadWrite', 'None')
        $lockVeltrix = [IO.File]::Open((Join-Path $installDir ".updater\$veltrixFilename"), 'Open', 'ReadWrite', 'None')
        try {
            for ($i = 1; $i -le 3; $i++) {
                $ver = $versions[$i]
                $payload = "MZ-dylan-$ver"
                $setupNext = Build-TestSetup -Version $ver -ExePayload $payload -Stable:$Stable
                $rUp = Start-Process -FilePath $setupNext -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
                if ($rUp.ExitCode -ne 0) {
                    $logTail = Get-Content -LiteralPath (Join-Path $installDir '.updater\setup.log') -Tail 10 -ErrorAction SilentlyContinue
                    throw "Actualización repetida a $ver falló (código $($rUp.ExitCode)). Log:`n$($logTail -join "`n")"
                }
                if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'version.txt')).Trim() -cne $ver) {
                    throw "version.txt no coincidió tras actualizar a $ver"
                }
                if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'dylan.exe')) -cne $payload) {
                    throw "dylan.exe no coincidió tras actualizar a $ver"
                }
                if ((Get-FileHash -LiteralPath $userFile -Algorithm SHA256).Hash -cne $userHash) {
                    throw "Los datos del usuario se alteraron al actualizar a $ver"
                }
                if ($Stable -and -not (Test-Path -LiteralPath (Join-Path $installDir 'Uninstall.exe'))) {
                    throw "Falta Uninstall.exe tras actualizar a $ver"
                }
            }
            Write-Output "OK [2/4]: 3 actualizaciones consecutivas con update.lock y $veltrixFilename bloqueados."

            # 3. Recuperación ante bloqueo transitorio de dylan.exe (y Uninstall.exe en modo estable)
            $verTransient = $versions[4]
            $payloadTransient = "MZ-dylan-$verTransient"
            $setupTransient = Build-TestSetup -Version $verTransient -ExePayload $payloadTransient -Stable:$Stable
            $exePath = Join-Path $installDir 'dylan.exe'
            $uninstPath = Join-Path $installDir 'Uninstall.exe'

            $lockSignal = Join-Path $workRoot ("lock-ready-" + [guid]::NewGuid().ToString('N') + '.flag')
            $locker = Start-Job -ScriptBlock {
                param($p1, $p2, $isStable, $signalPath)
                $f1 = [IO.File]::Open($p1, 'Open', 'ReadWrite', 'None')
                $f2 = $null
                try {
                    if ($isStable -and (Test-Path -LiteralPath $p2)) {
                        $f2 = [IO.File]::Open($p2, 'Open', 'ReadWrite', 'None')
                    }
                    Set-Content -LiteralPath $signalPath -Value 'locked' -NoNewline
                    Start-Sleep -Milliseconds 900
                } finally {
                    $f1.Dispose()
                    if ($f2) { $f2.Dispose() }
                }
            } -ArgumentList $exePath, $uninstPath, [bool]$Stable, $lockSignal

            $deadline = (Get-Date).AddSeconds(15)
            while (-not (Test-Path -LiteralPath $lockSignal)) {
                if ($locker.State -in @('Failed', 'Completed', 'Stopped')) {
                    Receive-Job $locker
                    throw 'El job de bloqueo transitorio terminó antes de señalar el bloqueo'
                }
                if ((Get-Date) -gt $deadline) {
                    throw 'Tiempo agotado esperando que el job de bloqueo adquiera el lock'
                }
                Start-Sleep -Milliseconds 50
            }
            $rTrans = Start-Process -FilePath $setupTransient -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
            Wait-Job $locker | Out-Null
            Receive-Job $locker | Out-Null
            Remove-Job $locker -Force
            Remove-Item -LiteralPath $lockSignal -Force -ErrorAction SilentlyContinue

            if ($rTrans.ExitCode -ne 0) {
                $logTail = Get-Content -LiteralPath (Join-Path $installDir '.updater\setup.log') -Tail 10 -ErrorAction SilentlyContinue
                throw "No se recuperó de bloqueo transitorio (código $($rTrans.ExitCode)). Log:`n$($logTail -join "`n")"
            }
            if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'version.txt')).Trim() -cne $verTransient) {
                throw "version.txt incorrecto tras recuperación de bloqueo transitorio"
            }
            Write-Output "OK [3/4]: Recuperación automática ante bloqueo transitorio (~900 ms) en dylan.exe$(if ($Stable) { ' y Uninstall.exe' })."

            if ($Stable) {
                $shortcutPath = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Dylan\Dylan.lnk'
                if (-not (Test-Path -LiteralPath $shortcutPath)) {
                    throw "No existe el acceso directo esperado antes de la prueba de bloqueo: $shortcutPath"
                }
                $verShortcut = '0.1.7-shortcut'
                $payloadShortcut = "MZ-dylan-$verShortcut"
                $setupShortcut = Build-TestSetup -Version $verShortcut -ExePayload $payloadShortcut -Stable
                $lockShortcut = [IO.File]::Open($shortcutPath, 'Open', 'ReadWrite', 'None')
                try {
                    $rSc = Start-Process -FilePath $setupShortcut -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
                } finally {
                    $lockShortcut.Dispose()
                }
                if ($rSc.ExitCode -ne 0) {
                    $logTail = Get-Content -LiteralPath (Join-Path $installDir '.updater\setup.log') -Tail 10 -ErrorAction SilentlyContinue
                    throw "El bloqueo del acceso directo no debe provocar fallo/rollback (código $($rSc.ExitCode)). Log:`n$($logTail -join "`n")"
                }
                if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'version.txt')).Trim() -cne $verShortcut) {
                    throw "version.txt no se actualizó cuando el acceso directo estuvo bloqueado"
                }
                $scLog = Get-Content -Raw -LiteralPath (Join-Path $installDir '.updater\setup.log')
                if ($scLog -notmatch 'event=warning op=create_shortcut\b') {
                    throw "setup.log no registró la advertencia de create_shortcut:`n$scLog"
                }
                $verTransient = $verShortcut
                $payloadTransient = $payloadShortcut
                Write-Output "OK [3b/4]: Bloqueo del acceso directo registrado como advertencia sin rollback."
            }

            # 4. Bloqueo permanente en version.txt -> rollback de dylan.exe + registro persistente con Win32=32
            $verFail = if ($Stable) { '0.1.8' } else { '1.0.0-beta.6' }
            $setupFail = Build-TestSetup -Version $verFail -ExePayload "MZ-dylan-$verFail" -Stable:$Stable
            $lockVersion = [IO.File]::Open((Join-Path $installDir 'version.txt'), 'Open', 'ReadWrite', 'None')
            try {
                $rFail = Start-Process -FilePath $setupFail -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
                if ($rFail.ExitCode -ne 22) {
                    throw "Se esperaba exit code 22 ante bloqueo permanente de version.txt, se obtuvo $($rFail.ExitCode)"
                }
            } finally {
                $lockVersion.Dispose()
            }

            if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'dylan.exe')) -cne $payloadTransient) {
                throw "Rollback falló: dylan.exe no conservó la versión anterior ($verTransient)"
            }
            if ((Get-Content -Raw -LiteralPath (Join-Path $installDir 'version.txt')).Trim() -cne $verTransient) {
                throw "version.txt se alteró inesperadamente tras rollback"
            }
            $setupLog = Get-Content -Raw -LiteralPath (Join-Path $installDir '.updater\setup.log')
            if ($setupLog -notmatch 'event=restore_start failed_op=replace_version_txt win32=(5|32|33)\b' -or
                $setupLog -notmatch 'event=failed op=replace_version_txt win32=(5|32|33) exit=22\b') {
                throw "setup.log no registró la operación fallida y el error Win32 esperado (5/32/33):`n$setupLog"
            }
            Write-Output "OK [4/4]: Rollback completo de dylan.exe y registro persistente verificado (op=replace_version_txt, win32=5/32/33, exit=22)."
        } finally {
            $lockUpdate.Dispose()
            $lockVeltrix.Dispose()
        }
    } finally {
        if (Test-Path -LiteralPath $installDir) {
            Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue
        }
        if ($Stable) {
            Remove-Item -Path 'HKCU:\Software\Dylan' -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Dylan' -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Dylan" -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

try {
    Run-Suite
    if ($TestStable) {
        Run-Suite -Stable
    }
    Write-Output 'TODAS LAS PRUEBAS DE ACTUALIZACIONES REPETIDAS NSIS PASARON.'
} finally {
    if (Test-Path -LiteralPath $workRoot) {
        Remove-Item -LiteralPath $workRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
