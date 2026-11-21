# Suite automatizada y aislada para verificar el comportamiento del instalador NSIS (setup.nsi).
# Utiliza archivos simulados (MZ-dylan-*, MZ-veltrix-*) dentro de una carpeta temporal única,
# claves de registro exclusivas de prueba y accesos directos temporales sin tocar instalaciones reales.
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

$runId = [guid]::NewGuid().ToString('N')
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$workRoot = [IO.Path]::GetFullPath((Join-Path $tempRoot "dylan-nsis-e2e-$runId"))
$testRegRoot = "Software\Dylan-NSIS-Test\run-$runId"

function Assert-IsolatedPath {
    param(
        [Parameter(Mandatory)][string]$CandidatePath,
        [Parameter(Mandatory)][string]$AllowedRoot
    )
    if ([string]::IsNullOrWhiteSpace($CandidatePath) -or -not [IO.Path]::IsPathRooted($CandidatePath)) {
        throw "Ruta no absoluta rechazada por aislamiento: '$CandidatePath'"
    }
    $fullCandidate = [IO.Path]::GetFullPath($CandidatePath).TrimEnd('\')
    $fullAllowed = [IO.Path]::GetFullPath($AllowedRoot).TrimEnd('\')
    $fullTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')

    if (-not $fullCandidate.StartsWith("$fullTemp\dylan-nsis-e2e-", [StringComparison]::OrdinalIgnoreCase)) {
        throw "Ruta fuera del árbol temporal aislado de pruebas NSIS: '$fullCandidate'"
    }
    if (-not ($fullCandidate.Equals($fullAllowed, [StringComparison]::OrdinalIgnoreCase) -or
              $fullCandidate.StartsWith("$fullAllowed\", [StringComparison]::OrdinalIgnoreCase))) {
        throw "Ruta '$fullCandidate' fuera de la raíz permitida '$fullAllowed'"
    }

    $realPrograms = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs')).TrimEnd('\')
    $realStartMenu = [IO.Path]::GetFullPath((Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu')).TrimEnd('\')
    if ($fullCandidate.StartsWith($realPrograms, [StringComparison]::OrdinalIgnoreCase) -or
        $fullCandidate.StartsWith($realStartMenu, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Operación abortada: intento de tocar una ruta de instalación real ('$fullCandidate')"
    }
}

function Remove-IsolatedDirectory {
    param(
        [Parameter(Mandatory)][string]$TargetPath,
        [Parameter(Mandatory)][string]$AllowedRoot
    )
    Assert-IsolatedPath -CandidatePath $TargetPath -AllowedRoot $AllowedRoot
    if (Test-Path -LiteralPath $TargetPath) {
        Remove-Item -LiteralPath $TargetPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Remove-IsolatedRegistryTree {
    param([Parameter(Mandatory)][string]$RegSubKey)
    if ($RegSubKey -cnotmatch '^Software\\Dylan-NSIS-Test\\run-[0-9a-f]{32}(\\|$)') {
        throw "Clave de registro fuera del espacio aislado de pruebas: '$RegSubKey'"
    }
    $regPath = "HKCU:\$RegSubKey"
    if (Test-Path -LiteralPath $regPath) {
        Remove-Item -LiteralPath $regPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Assert-IsolatedPath -CandidatePath $workRoot -AllowedRoot $workRoot
New-Item -ItemType Directory -Force $workRoot | Out-Null

function Build-TestSetup {
    param(
        [string]$Version,
        [string]$ExePayload,
        [string]$ParentDir,
        [string]$InstallDir,
        [string]$ShortcutDir,
        [string]$RegRoot,
        [string]$MutexSuffix,
        [switch]$Stable
    )
    Assert-IsolatedPath -CandidatePath $ParentDir -AllowedRoot $workRoot
    Assert-IsolatedPath -CandidatePath $InstallDir -AllowedRoot $ParentDir
    Assert-IsolatedPath -CandidatePath $ShortcutDir -AllowedRoot $workRoot

    $buildDir = Join-Path $workRoot ("build-" + $Version + $(if ($Stable) { '-stable' } else { '-beta' }))
    $inputs = Join-Path $buildDir 'inputs'
    New-Item -ItemType Directory -Force $inputs | Out-Null

    # Archivos simulados para probar el comportamiento de NSIS sin usar binarios reales.
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
        "/DVELTRIX_FILENAME=$veltrixFilename",
        '/DTEST_ISOLATION=1',
        "/DTEST_PARENT_DIR=$ParentDir",
        "/DTEST_INSTALL_DIR=$InstallDir",
        "/DTEST_SHORTCUT_DIR=$ShortcutDir",
        "/DTEST_REG_ROOT=$RegRoot",
        "/DTEST_MUTEX_SUFFIX=$MutexSuffix"
    )
    if ($Stable) { $args += '/DAPP_STABLE=1' }
    $args += $setupNsi

    & $MakeNsis @args
    if ($LASTEXITCODE) { throw "Falló MakeNSIS para $Version" }
    return (Join-Path $buildDir "Dylan-Setup-$Version-x64.exe")
}

function Run-Suite {
    param([switch]$Stable)

    $modeTag = if ($Stable) { 'stable' } else { 'beta' }
    $modeLabel = if ($Stable) { 'STABLE (Dylan + Uninstall.exe en entorno aislado)' } else { 'BETA (Dylan-Test-Veltrix en entorno aislado)' }
    $appDirName = if ($Stable) { 'Dylan' } else { 'Dylan-Test-Veltrix' }
    $suiteRoot = Join-Path $workRoot "suite-$modeTag"
    $parentDir = Join-Path $suiteRoot 'Programs'
    $installDir = Join-Path $parentDir $appDirName
    $shortcutDir = Join-Path $suiteRoot "StartMenu\$appDirName"
    $suiteRegRoot = "$testRegRoot\$modeTag"
    $mutexSuffix = "$runId-$modeTag"

    Assert-IsolatedPath -CandidatePath $suiteRoot -AllowedRoot $workRoot
    Assert-IsolatedPath -CandidatePath $parentDir -AllowedRoot $suiteRoot
    Assert-IsolatedPath -CandidatePath $installDir -AllowedRoot $parentDir
    Assert-IsolatedPath -CandidatePath $shortcutDir -AllowedRoot $suiteRoot
    New-Item -ItemType Directory -Force $parentDir | Out-Null

    $versions = if ($Stable) {
        @('0.1.3', '0.1.4', '0.1.5', '0.1.6', '0.1.7')
    } else {
        @('1.0.0-beta.1', '1.0.0-beta.2', '1.0.0-beta.3', '1.0.0-beta.4', '1.0.0-beta.5')
    }

    $buildCommon = @{
        ParentDir   = $parentDir
        InstallDir  = $installDir
        ShortcutDir = $shortcutDir
        RegRoot     = $suiteRegRoot
        MutexSuffix = $mutexSuffix
        Stable      = $Stable
    }

    Write-Output "=== Suite aislada de comportamiento NSIS (archivos simulados): $modeLabel ==="

    try {
        # 1. Rechazo de ruta destino no permitida (seguridad)
        $setup0 = Build-TestSetup -Version $versions[0] -ExePayload "MZ-dylan-$($versions[0])" @buildCommon
        $foreignDir = Join-Path $suiteRoot 'destino no permitido'
        $r0 = Start-Process -FilePath $setup0 -ArgumentList @('/S', "/D=$foreignDir") -Wait -PassThru -WindowStyle Hidden
        if ($r0.ExitCode -ne 20 -or (Test-Path -LiteralPath $foreignDir)) {
            throw "NSIS no rechazó destino ajeno (código $($r0.ExitCode))"
        }
        Write-Output "OK [1]: Rechazo de ruta destino ajena (exit=20)."

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
                $setupNext = Build-TestSetup -Version $ver -ExePayload $payload @buildCommon
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
            Write-Output "OK [2]: 3 actualizaciones consecutivas con update.lock y $veltrixFilename bloqueados."

            # 3. Recuperación ante bloqueo transitorio de dylan.exe (y Uninstall.exe en modo estable)
            $verTransient = $versions[4]
            $payloadTransient = "MZ-dylan-$verTransient"
            $setupTransient = Build-TestSetup -Version $verTransient -ExePayload $payloadTransient @buildCommon
            $exePath = Join-Path $installDir 'dylan.exe'
            $uninstPath = Join-Path $installDir 'Uninstall.exe'

            $lockSignal = Join-Path $suiteRoot ("lock-ready-" + [guid]::NewGuid().ToString('N') + '.flag')
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
            Write-Output "OK [3]: Recuperación automática ante bloqueo transitorio (~900 ms) en dylan.exe$(if ($Stable) { ' y Uninstall.exe' })."

            if ($Stable) {
                $shortcutPath = Join-Path $shortcutDir 'Dylan.lnk'
                if (-not (Test-Path -LiteralPath $shortcutPath)) {
                    throw "No existe el acceso directo aislado esperado antes de la prueba de bloqueo: $shortcutPath"
                }
                $verShortcut = '0.1.7-shortcut'
                $payloadShortcut = "MZ-dylan-$verShortcut"
                $setupShortcut = Build-TestSetup -Version $verShortcut -ExePayload $payloadShortcut @buildCommon
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
                Write-Output "OK [3b]: Bloqueo del acceso directo temporal registrado como advertencia sin rollback."
            }

            # 4. Bloqueo permanente en version.txt -> rollback exitoso (exit=22)
            $verFail = if ($Stable) { '0.1.8' } else { '1.0.0-beta.6' }
            $setupFail = Build-TestSetup -Version $verFail -ExePayload "MZ-dylan-$verFail" @buildCommon
            $lockVersion = [IO.File]::Open((Join-Path $installDir 'version.txt'), 'Open', 'ReadWrite', 'None')
            try {
                $rFail = Start-Process -FilePath $setupFail -ArgumentList @('/S', "/D=$installDir") -Wait -PassThru -WindowStyle Hidden
                if ($rFail.ExitCode -ne 22) {
                    throw "Se esperaba exit code 22 ante bloqueo permanente de version.txt con restauración exitosa, se obtuvo $($rFail.ExitCode)"
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
                throw "setup.log no registró la operación fallida y exit=22 esperados:`n$setupLog"
            }
            Write-Output "OK [4]: Rollback completo de dylan.exe y registro persistente verificado (op=replace_version_txt, exit=22)."

            # 5. En modo estable: fallo al restaurar Uninstall.exe -> conserva respaldo, restaura dylan.exe igualmente y devuelve 23
            if ($Stable) {
                $verFailUninst = '0.1.9'
                $setupFailUninst = Build-TestSetup -Version $verFailUninst -ExePayload "MZ-dylan-$verFailUninst" @buildCommon
                $lockVersion2 = [IO.File]::Open((Join-Path $installDir 'version.txt'), 'Open', 'ReadWrite', 'None')
                $lockUninst = $null
                try {
                    $proc = Start-Process -FilePath $setupFailUninst -ArgumentList @('/S', "/D=$installDir") -PassThru -WindowStyle Hidden
                    # Esperar a que Setup reemplace dylan.exe y Uninstall.exe y entre en los reintentos de replace_version_txt (~10 s).
                    $waitDeadline = (Get-Date).AddSeconds(8)
                    while ((Get-Date) -lt $waitDeadline) {
                        $currentExeContent = Get-Content -Raw -LiteralPath $exePath -ErrorAction SilentlyContinue
                        if ($currentExeContent -ceq "MZ-dylan-$verFailUninst") {
                            Start-Sleep -Milliseconds 400
                            break
                        }
                        Start-Sleep -Milliseconds 100
                    }
                    $lockUninst = [IO.File]::Open($uninstPath, 'Open', 'ReadWrite', 'None')
                    $proc.WaitForExit()
                    if ($proc.ExitCode -ne 23) {
                        $logTail = Get-Content -LiteralPath (Join-Path $installDir '.updater\setup.log') -Tail 15 -ErrorAction SilentlyContinue
                        throw "Se esperaba exit code 23 cuando falla la restauración de Uninstall.exe, se obtuvo $($proc.ExitCode). Log:`n$($logTail -join "`n")"
                    }
                } finally {
                    if ($lockUninst) { $lockUninst.Dispose() }
                    $lockVersion2.Dispose()
                }

                if ((Get-Content -Raw -LiteralPath $exePath) -cne $payloadTransient) {
                    throw "dylan.exe no se restauró cuando falló la restauración de Uninstall.exe"
                }
                $uninstLog = Get-Content -Raw -LiteralPath (Join-Path $installDir '.updater\setup.log')
                if ($uninstLog -notmatch 'event=restore_uninstaller_failed win32=(5|32|33) backup=([^\r\n]+)') {
                    throw "setup.log no registró la ubicación del respaldo de Uninstall.exe:`n$uninstLog"
                }
                $backupUninstPath = $Matches[2].Trim()
                if (-not (Test-Path -LiteralPath $backupUninstPath) -or (Get-Item -LiteralPath $backupUninstPath).Length -lt 1) {
                    throw "El respaldo de Uninstall.exe no se conservó en '$backupUninstPath'"
                }
                if ($uninstLog -notmatch 'event=rollback_failed op=replace_version_txt win32=(5|32|33) exit=23\b') {
                    throw "setup.log no registró event=rollback_failed con exit=23:`n$uninstLog"
                }
                Write-Output "OK [5]: Fallo al restaurar Uninstall.exe devuelve exit=23, conserva respaldo ($backupUninstPath) y restaura dylan.exe."
            }
        } finally {
            $lockUpdate.Dispose()
            $lockVeltrix.Dispose()
        }
    } finally {
        Remove-IsolatedDirectory -TargetPath $suiteRoot -AllowedRoot $workRoot
        if ($Stable) {
            Remove-IsolatedRegistryTree -RegSubKey $suiteRegRoot
        }
    }
}

try {
    Run-Suite
    if ($TestStable) {
        Run-Suite -Stable
    }
    Write-Output 'TODAS LAS PRUEBAS AISLADAS DE COMPORTAMIENTO NSIS PASARON.'
} finally {
    Remove-IsolatedDirectory -TargetPath $workRoot -AllowedRoot $workRoot
    Remove-IsolatedRegistryTree -RegSubKey $testRegRoot
}
