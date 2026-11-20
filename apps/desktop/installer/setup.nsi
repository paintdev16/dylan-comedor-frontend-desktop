Unicode true
!include "MUI2.nsh"

!ifndef APP_VERSION
  !error "APP_VERSION es obligatoria"
!endif
!ifndef PILOT_INPUT
  !error "PILOT_INPUT debe apuntar a los binarios y al marcador de versión"
!endif
!ifndef OUT_DIR
  !error "OUT_DIR es obligatorio"
!endif
!ifndef VELTRIX_FILENAME
  !error "VELTRIX_FILENAME es obligatorio"
!endif

!ifdef APP_STABLE
  !define APP_NAME "Dylan"
  !define APP_DIR "Dylan"
  !define INSTALL_KEY "Software\Dylan"
  !define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Dylan"
!else
  !define APP_NAME "Dylan Test"
  !define APP_DIR "Dylan-Test-Veltrix"
!endif

Name "${APP_NAME}"
OutFile "${OUT_DIR}\Dylan-Setup-${APP_VERSION}-x64.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP_DIR}"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!ifdef APP_STABLE
  !insertmacro MUI_UNPAGE_CONFIRM
  !insertmacro MUI_UNPAGE_INSTFILES
!endif
!insertmacro MUI_LANGUAGE "Spanish"

Var NewExe
Var NewVersion
Var BackupExe
Var FailedOp
Var FailedWinError
Var LastWinError
!ifdef APP_STABLE
Var PreviousVersion
Var NewUninstaller
Var BackupUninstaller
!endif

Function .onInit
    ; Mantener el handle hasta que termine Setup, incluso con /S.
    System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Local\${APP_DIR}.Setup") p .r0 ?e'
    Pop $1
    StrCmp $0 0 busy
    StrCmp $1 183 busy
    ; NSIS aplica /D antes de .onInit. El instalador solo escribe en su propia carpeta.
    StrCpy $0 "$LOCALAPPDATA\Programs\${APP_DIR}"
    StrCmp $0 "$INSTDIR" 0 wrong_dir
!ifdef APP_STABLE
    ReadRegStr $1 HKCU "${INSTALL_KEY}" "InstallDir"
    StrCmp $1 "" +2
    StrCmp $1 "$INSTDIR" 0 wrong_dir
!endif
    ; Rechazar redirecciones en el árbol que puede afectar a la instalación.
    Push "$LOCALAPPDATA\Programs"
    Call CheckDestinationDirectory
    Push "$INSTDIR"
    Call CheckDestinationDirectory
    Push "$INSTDIR\.updater"
    Call CheckDestinationDirectory
    Goto done
    wrong_dir:
    SetErrorLevel 20
    IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "Setup solo puede instalar ${APP_NAME} en $0."
    Quit
    busy:
    SetErrorLevel 21
    IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "Ya se está ejecutando otro Setup de ${APP_NAME}."
    Quit
    done:
FunctionEnd

Function CheckDestinationDirectory
    Pop $2
    System::Call 'kernel32::GetFileAttributesW(w "$2") i .r3'
    IntCmp $3 -1 accepted
    IntOp $4 $3 & 0x410 ; Debe ser directorio y no un punto de reanálisis.
    IntCmp $4 16 accepted
    SetErrorLevel 20
    IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "Destino no permitido: $2"
    Quit
    accepted:
FunctionEnd

; Registra un evento persistente en $INSTDIR\.updater\setup.log sin alterar el flujo ni ocultar errores.
Function LogSetup
    Exch $R0 ; mensaje
    Push $R1
    Push $R2
    Push $R3
    Push $R4
    Push $R5
    Push $R6
    Push $R7
    Push $R8
    ClearErrors
    IfFileExists "$INSTDIR\.updater\*.*" 0 log_done
    System::Call 'kernel32::GetCurrentProcessId() i .R1'
    System::Call '*(&i2, &i2, &i2, &i2, &i2, &i2, &i2, &i2) p .R2'
    StrCmp $R2 0 log_no_time
    System::Call 'kernel32::GetLocalTime(p R2)'
    System::Call '*$R2(&i2.R3, &i2.R4, &i2, &i2.R5, &i2.R6, &i2.R7, &i2.R8)'
    System::Free $R2
    StrCpy $R2 "$R3-$R4-$R5 $R6:$R7:$R8"
    Goto log_write
    log_no_time:
    StrCpy $R2 "0000-00-00 00:00:00"
    log_write:
    ClearErrors
    FileOpen $R3 "$INSTDIR\.updater\setup.log" a
    IfErrors log_done
    FileSeek $R3 0 END
    FileWrite $R3 "[$R2] pid=$R1 ver=${APP_VERSION} $R0$\r$\n"
    FileClose $R3
    log_done:
    ClearErrors
    Pop $R8
    Pop $R7
    Pop $R6
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Pop $R1
    Pop $R0
FunctionEnd

; Copia un archivo con reintentos solo ante errores transitorios de bloqueo (5, 32, 33).
; Entrada en pila: Push origen, Push destino. Salida: $0 = 1 (OK) o 0 (error), $LastWinError = Win32 error.
Function CopyFileRetry
    Pop $2 ; destino
    Pop $1 ; origen
    StrCpy $3 50
    StrCpy $LastWinError "0"
    loop:
    System::Call 'kernel32::CopyFileW(w "$1", w "$2", i 0) i .r0 ?e'
    Pop $4
    StrCmp $0 0 0 ok
    StrCpy $LastWinError "$4"
    IntCmp $4 5 transient
    IntCmp $4 32 transient
    IntCmp $4 33 transient
    Goto fail
    transient:
    IntOp $3 $3 - 1
    IntCmp $3 0 fail fail
    Sleep 200
    Goto loop
    fail:
    StrCpy $0 0
    Goto done
    ok:
    StrCpy $LastWinError "0"
    StrCpy $0 1
    done:
FunctionEnd

; Mueve/reemplaza un archivo atómicamente con reintentos ante errores transitorios de bloqueo (5, 32, 33).
; Entrada en pila: Push origen, Push destino. Salida: $0 = 1 (OK) o 0 (error), $LastWinError = Win32 error.
Function MoveFileRetry
    Pop $2 ; destino
    Pop $1 ; origen
    StrCpy $3 50
    StrCpy $LastWinError "0"
    loop:
    System::Call 'kernel32::MoveFileExW(w "$1", w "$2", i 9) i .r0 ?e'
    Pop $4
    StrCmp $0 0 0 ok
    StrCpy $LastWinError "$4"
    IntCmp $4 5 transient
    IntCmp $4 32 transient
    IntCmp $4 33 transient
    Goto fail
    transient:
    IntOp $3 $3 - 1
    IntCmp $3 0 fail fail
    Sleep 200
    Goto loop
    fail:
    StrCpy $0 0
    Goto done
    ok:
    StrCpy $LastWinError "0"
    StrCpy $0 1
    done:
FunctionEnd

Section "Instalar ${APP_NAME}"
    ; Aprovisionar una sola vez. Veltrix mantiene update.lock abierto durante /S.
    StrCpy $FailedOp "create_updater_dir"
    StrCpy $LastWinError "0"
    ClearErrors
    SetOutPath "$INSTDIR\.updater"
    IfErrors failed

    Push "event=start dir=$INSTDIR"
    Call LogSetup

    SetOverwrite off
    StrCpy $FailedOp "extract_app_json"
    ClearErrors
    IfFileExists "$INSTDIR\.updater\app.json" +2
    File "${PILOT_INPUT}\app.json"
    IfErrors failed

    ; Una migración puede ejecutarse desde el Veltrix anterior, que sigue activo.
    StrCpy $FailedOp "extract_veltrix"
    ClearErrors
    IfFileExists "$INSTDIR\.updater\${VELTRIX_FILENAME}" +2
    File /oname=${VELTRIX_FILENAME} "${PILOT_INPUT}\veltrix.exe"
    IfErrors failed

    IfFileExists "$INSTDIR\.updater\update.lock" prepare
    StrCpy $FailedOp "create_update_lock"
    ClearErrors
    FileOpen $0 "$INSTDIR\.updater\update.lock" w
    IfErrors failed
    FileClose $0
    IfErrors failed

    prepare:
    StrCpy $FailedOp "prepare_temp_files"
    ClearErrors
    SetOutPath "$INSTDIR"
    IfErrors failed
    GetTempFileName $NewExe "$INSTDIR"
    IfErrors failed
    GetTempFileName $NewVersion "$INSTDIR"
    IfErrors failed
    SetOverwrite on

    StrCpy $FailedOp "extract_dylan_exe"
    ClearErrors
    File /oname=$NewExe "${PILOT_INPUT}\dylan.exe"
    IfErrors failed

    StrCpy $FailedOp "extract_version_txt"
    ClearErrors
    File /oname=$NewVersion "${PILOT_INPUT}\version.txt"
    IfErrors failed

    ; Sustituir solo el ejecutable y el marcador. Nunca tocar datos ni .updater.
    IfFileExists "$INSTDIR\dylan.exe" 0 replace
    StrCpy $FailedOp "backup_dylan_exe"
    ClearErrors
    GetTempFileName $BackupExe "$INSTDIR"
    IfErrors failed
    Push "$INSTDIR\dylan.exe"
    Push "$BackupExe"
    Call CopyFileRetry
    StrCmp $0 0 failed

    replace:
    StrCpy $FailedOp "replace_dylan_exe"
    Push "$NewExe"
    Push "$INSTDIR\dylan.exe"
    Call MoveFileRetry
    StrCmp $0 0 failed
!ifdef APP_STABLE
    ReadRegStr $PreviousVersion HKCU "${UNINSTALL_KEY}" "DisplayVersion"

    ; Actualizar Uninstall.exe mediante archivo temporal y reemplazo con reintentos.
    IfFileExists "$INSTDIR\Uninstall.exe" 0 write_uninst
    StrCpy $FailedOp "backup_uninstaller"
    ClearErrors
    GetTempFileName $BackupUninstaller "$INSTDIR"
    IfErrors restore
    Push "$INSTDIR\Uninstall.exe"
    Push "$BackupUninstaller"
    Call CopyFileRetry
    StrCmp $0 0 restore

    write_uninst:
    StrCpy $FailedOp "write_uninstaller_temp"
    ClearErrors
    GetTempFileName $NewUninstaller "$INSTDIR"
    IfErrors restore
    WriteUninstaller "$NewUninstaller"
    IfErrors restore

    StrCpy $FailedOp "replace_uninstaller"
    Push "$NewUninstaller"
    Push "$INSTDIR\Uninstall.exe"
    Call MoveFileRetry
    StrCmp $0 0 restore

    StrCpy $FailedOp "register_stable_metadata"
    ClearErrors
    WriteRegStr HKCU "${INSTALL_KEY}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\dylan.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
    IfErrors restore

    ; Los accesos directos son no críticos: reintentar brevemente y registrar advertencias sin provocar rollback.
    StrCpy $3 5
    shortcut_dir_loop:
    ClearErrors
    CreateDirectory "$SMPROGRAMS\${APP_DIR}"
    IfErrors 0 shortcut_dir_ok
    System::Call 'kernel32::GetLastError() i .r0'
    IntOp $3 $3 - 1
    IntCmp $3 0 shortcut_dir_warn shortcut_dir_warn
    Sleep 200
    Goto shortcut_dir_loop

    shortcut_dir_warn:
    Push "event=warning op=create_shortcut_dir win32=$0 path=$SMPROGRAMS\${APP_DIR}"
    Call LogSetup
    ClearErrors
    Goto shortcut_done

    shortcut_dir_ok:
    StrCpy $3 5
    shortcut_link_loop:
    ClearErrors
    CreateShortcut "$SMPROGRAMS\${APP_DIR}\Dylan.lnk" "$INSTDIR\dylan.exe"
    IfErrors 0 shortcut_done
    System::Call 'kernel32::GetLastError() i .r0'
    IntOp $3 $3 - 1
    IntCmp $3 0 shortcut_link_warn shortcut_link_warn
    Sleep 200
    Goto shortcut_link_loop

    shortcut_link_warn:
    Push "event=warning op=create_shortcut win32=$0 path=$SMPROGRAMS\${APP_DIR}\Dylan.lnk"
    Call LogSetup
    ClearErrors

    shortcut_done:
!endif
    StrCpy $FailedOp "replace_version_txt"
    Push "$NewVersion"
    Push "$INSTDIR\version.txt"
    Call MoveFileRetry
    StrCmp $0 0 restore

    StrCmp $BackupExe "" +2
    Delete "$BackupExe"
!ifdef APP_STABLE
    StrCmp $BackupUninstaller "" +2
    Delete "$BackupUninstaller"
!endif
    Push "event=success exit=0"
    Call LogSetup
    SetErrorLevel 0
    Goto done

    restore:
    StrCpy $FailedWinError "$LastWinError"
    Push "event=restore_start failed_op=$FailedOp win32=$FailedWinError"
    Call LogSetup
!ifdef APP_STABLE
    StrCmp $PreviousVersion "" +3
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "$PreviousVersion"
    Goto +2
    DeleteRegValue HKCU "${UNINSTALL_KEY}" "DisplayVersion"
    StrCmp $BackupUninstaller "" restore_exe
    Push "$BackupUninstaller"
    Push "$INSTDIR\Uninstall.exe"
    Call MoveFileRetry
    StrCmp $0 0 0 restore_exe
    Push "event=restore_uninstaller_failed win32=$LastWinError"
    Call LogSetup
    restore_exe:
!endif
    StrCmp $BackupExe "" remove_new
    Push "$BackupExe"
    Push "$INSTDIR\dylan.exe"
    Call MoveFileRetry
    StrCmp $0 0 rollback_failed failed_restored
    remove_new:
    ClearErrors
    Delete "$INSTDIR\dylan.exe"
    IfErrors rollback_failed failed_restored

    failed_restored:
    StrCpy $LastWinError "$FailedWinError"

    failed:
    Push "event=failed op=$FailedOp win32=$LastWinError exit=22"
    Call LogSetup
    StrCmp $BackupExe "" +2
    Delete "$BackupExe"
    StrCmp $NewExe "" +2
    Delete "$NewExe"
    StrCmp $NewVersion "" +2
    Delete "$NewVersion"
!ifdef APP_STABLE
    StrCmp $NewUninstaller "" +2
    Delete "$NewUninstaller"
    StrCmp $BackupUninstaller "" +2
    Delete "$BackupUninstaller"
!endif
    SetErrorLevel 22
    IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "No se pudo escribir la instalación ($FailedOp, Win32=$LastWinError). Cierra ${APP_NAME} y comprueba los permisos."
    Quit

    rollback_failed:
    Push "event=rollback_failed op=$FailedOp win32=$LastWinError exit=23 backup=$BackupExe"
    Call LogSetup
    SetErrorLevel 23
    IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "No se pudo restaurar ${APP_NAME} (Win32=$LastWinError). Copia disponible: $BackupExe"
    Quit
    done:
SectionEnd

!ifdef APP_STABLE
Section "Uninstall"
    ClearErrors
    Delete "$INSTDIR\dylan.exe"
    IfErrors 0 +3
    MessageBox MB_OK|MB_ICONSTOP "Cierra Dylan antes de desinstalarlo." /SD IDOK
    Abort
    Delete "$INSTDIR\version.txt"
    Delete "$INSTDIR\.updater\veltrix.exe"
    Delete "$INSTDIR\.updater\${VELTRIX_FILENAME}"
    Delete "$INSTDIR\.updater\app.json"
    Delete "$INSTDIR\.updater\update.lock"
    Delete "$INSTDIR\.updater\setup.log"
    Delete "$INSTDIR\Uninstall.exe"
    Delete "$SMPROGRAMS\${APP_DIR}\Dylan.lnk"
    RMDir "$SMPROGRAMS\${APP_DIR}"
    RMDir "$INSTDIR\.updater"
    RMDir "$INSTDIR"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    DeleteRegValue HKCU "${INSTALL_KEY}" "InstallDir"
SectionEnd
!endif
