; =============================================================================
; AEGIS — язык программирования + IDE. Чистый x86-64, FASM, без CRT.
; =============================================================================
if defined AEGIS_CONSOLE
        format PE64 console 5.0
else
        format PE64 GUI 5.0
end if
stack 0x200000,0x200000
entry start

section '.text' code readable executable

start:
        sub rsp,0x28
        call [GetCommandLineW]
        mov rcx,rax
        lea rdx,[g_argc]
        mov rax,[CommandLineToArgvW]
        call wcall4
        test rax,rax
        jz .gui
        mov rsi,rax
        cmp dword [g_argc],2
        jb .gui
        call cli_main
        mov ecx,eax
        call [ExitProcess]
.gui:   call ide_main
        xor ecx,ecx
        call [ExitProcess]

include 'core/wcall.asm'
include 'core/heap.asm'
include 'core/crypto.asm'
include 'core/lz.asm'
include 'core/cursed.asm'
include 'core/codec0.asm'
include 'core/compiler.asm'
include 'cli.asm'
include 'ide/ide.asm'
include 'data.asm'
; --- таблица импортов: пишем вручную (без внешних include-макросов) ---
section '.idata' import data readable writeable

id_dir:
  dd 0,0,0,RVA n_kernel, RVA t_kernel
  dd 0,0,0,RVA n_user,   RVA t_user
  dd 0,0,0,RVA n_shell,  RVA t_shell
  dd 0,0,0,RVA n_adv,    RVA t_adv
  dd 0,0,0,RVA n_nt,     RVA t_nt
  dd 0,0,0,RVA n_comdlg, RVA t_comdlg
  dd 0,0,0,RVA n_gdi,    RVA t_gdi
  dd 0,0,0,RVA n_dwm,    RVA t_dwm
  dd 0,0,0,RVA n_theme,  RVA t_theme
  dd 0,0,0,0,0

t_kernel:
  ExitProcess          dq RVA h_ExitProcess
  GetCommandLineW      dq RVA h_GetCommandLineW
  VirtualAlloc         dq RVA h_VirtualAlloc
  VirtualFree          dq RVA h_VirtualFree
  CreateFileW          dq RVA h_CreateFileW
  ReadFile             dq RVA h_ReadFile
  WriteFile            dq RVA h_WriteFile
  CloseHandle          dq RVA h_CloseHandle
  SetFilePointer       dq RVA h_SetFilePointer
  AttachConsole        dq RVA h_AttachConsole
  GetStdHandle         dq RVA h_GetStdHandle
  LoadLibraryA         dq RVA h_LoadLibraryA
  GetModuleHandleW     dq RVA h_GetModuleHandleW
  GetTickCount64       dq RVA h_GetTickCount64
  QueryPerformanceCounter dq RVA h_QueryPerformanceCounter
  WideCharToMultiByte  dq RVA h_WideCharToMultiByte
  MultiByteToWideChar  dq RVA h_MultiByteToWideChar
  GetVolumeInformationW dq RVA h_GetVolumeInformationW
  LocalFree            dq RVA h_LocalFree
  GlobalAlloc          dq RVA h_GlobalAlloc
  GlobalFree           dq RVA h_GlobalFree
  GetComputerNameW     dq RVA h_GetComputerNameW
  GetLastError         dq RVA h_GetLastError
  Sleep                dq RVA h_Sleep
  DeleteFileW          dq RVA h_DeleteFileW
  AddVectoredExceptionHandler dq RVA h_AVEH
  CreateProcessW       dq RVA h_CreateProcessW
  CreatePipe           dq RVA h_CreatePipe
  SetHandleInformation dq RVA h_SetHandleInformation
  PeekNamedPipe        dq RVA h_PeekNamedPipe
  GetExitCodeProcess   dq RVA h_GetExitCodeProcess
  TerminateProcess     dq RVA h_TerminateProcess
  GetFullPathNameW     dq RVA h_GetFullPathNameW
  GetTempFileNameW     dq RVA h_GetTempFileNameW
  MoveFileExW          dq RVA h_MoveFileExW
  FlushFileBuffers     dq RVA h_FlushFileBuffers
  WaitForSingleObject  dq RVA h_WaitForSingleObject
  dq 0

t_user:
  MessageBoxA          dq RVA h_MessageBoxA
  MessageBoxW          dq RVA h_MessageBoxW
  CreateWindowExW      dq RVA h_CreateWindowExW
  DefWindowProcW       dq RVA h_DefWindowProcW
  RegisterClassExW     dq RVA h_RegisterClassExW
  GetMessageW          dq RVA h_GetMessageW
  TranslateMessage     dq RVA h_TranslateMessage
  DispatchMessageW     dq RVA h_DispatchMessageW
  PostQuitMessage      dq RVA h_PostQuitMessage
  SendMessageW         dq RVA h_SendMessageW
  SetWindowTextW       dq RVA h_SetWindowTextW
  GetWindowTextW       dq RVA h_GetWindowTextW
  GetWindowTextLengthW dq RVA h_GetWindowTextLengthW
  SetTimer             dq RVA h_SetTimer
  KillTimer            dq RVA h_KillTimer
  DestroyWindow        dq RVA h_DestroyWindow
  ShowWindow           dq RVA h_ShowWindow
  UpdateWindow         dq RVA h_UpdateWindow
  SetFocus             dq RVA h_SetFocus
  InvalidateRect       dq RVA h_InvalidateRect
  LoadCursorW          dq RVA h_LoadCursorW
  LoadIconW            dq RVA h_LoadIconW
  PostMessageW         dq RVA h_PostMessageW
  MoveWindow           dq RVA h_MoveWindow
  GetClientRect        dq RVA h_GetClientRect
  BeginPaint           dq RVA h_BeginPaint
  EndPaint             dq RVA h_EndPaint
  FillRect             dq RVA h_FillRect
  DrawTextW            dq RVA h_DrawTextW
  CreateAcceleratorTableW dq RVA h_CreateAcceleratorTableW
  TranslateAcceleratorW dq RVA h_TranslateAcceleratorW
  DestroyAcceleratorTable dq RVA h_DestroyAcceleratorTable
  SetProcessDPIAware   dq RVA h_SetProcessDPIAware
  dq 0

t_shell:
  CommandLineToArgvW   dq RVA h_CommandLineToArgvW
  dq 0

t_adv:
  SystemFunction036    dq RVA h_SystemFunction036
  GetUserNameA         dq RVA h_GetUserNameA
  dq 0

t_nt:
  NtQueryInformationProcess dq RVA h_NtQueryInformationProcess
  dq 0

t_comdlg:
  GetOpenFileNameW     dq RVA h_GetOpenFileNameW
  GetSaveFileNameW     dq RVA h_GetSaveFileNameW
  dq 0

t_gdi:
  CreateFontW          dq RVA h_CreateFontW
  DeleteObject         dq RVA h_DeleteObject
  CreateFontIndirectW  dq RVA h_CreateFontIndirectW
  CreateSolidBrush     dq RVA h_CreateSolidBrush
  SetTextColor         dq RVA h_SetTextColor
  SetBkColor           dq RVA h_SetBkColor
  SetBkMode            dq RVA h_SetBkMode
  SelectObject         dq RVA h_SelectObject
  dq 0

t_dwm:
  DwmSetWindowAttribute dq RVA h_DwmSetWindowAttribute
  dq 0
t_theme:
  SetWindowTheme       dq RVA h_SetWindowTheme
  dq 0

n_kernel  db 'KERNEL32.DLL',0
n_user    db 'USER32.DLL',0
n_shell   db 'SHELL32.DLL',0
n_adv     db 'ADVAPI32.DLL',0
n_nt      db 'NTDLL.DLL',0
n_comdlg  db 'COMDLG32.DLL',0
n_gdi     db 'GDI32.DLL',0
n_dwm     db 'DWMAPI.DLL',0
n_theme   db 'UXTHEME.DLL',0

macro HINT n { dw 0
  db n,0 }
h_ExitProcess:            HINT 'ExitProcess'
h_GetCommandLineW:        HINT 'GetCommandLineW'
h_VirtualAlloc:           HINT 'VirtualAlloc'
h_VirtualFree:            HINT 'VirtualFree'
h_CreateFileW:            HINT 'CreateFileW'
h_ReadFile:               HINT 'ReadFile'
h_WriteFile:              HINT 'WriteFile'
h_CloseHandle:            HINT 'CloseHandle'
h_SetFilePointer:         HINT 'SetFilePointer'
h_AttachConsole:          HINT 'AttachConsole'
h_GetStdHandle:           HINT 'GetStdHandle'
h_LoadLibraryA:           HINT 'LoadLibraryA'
h_GetModuleHandleW:       HINT 'GetModuleHandleW'
h_GetTickCount64:         HINT 'GetTickCount64'
h_QueryPerformanceCounter: HINT 'QueryPerformanceCounter'
h_WideCharToMultiByte:    HINT 'WideCharToMultiByte'
h_MultiByteToWideChar:    HINT 'MultiByteToWideChar'
h_GetVolumeInformationW: HINT 'GetVolumeInformationW'
h_LocalFree:              HINT 'LocalFree'
h_GlobalAlloc:            HINT 'GlobalAlloc'
h_GlobalFree:             HINT 'GlobalFree'
h_GetComputerNameW:       HINT 'GetComputerNameW'
h_GetLastError:           HINT 'GetLastError'
h_Sleep:                  HINT 'Sleep'
h_DeleteFileW:             HINT 'DeleteFileW'
h_AVEH:                   HINT 'AddVectoredExceptionHandler'
h_MessageBoxA:            HINT 'MessageBoxA'
h_MessageBoxW:            HINT 'MessageBoxW'
h_CreateWindowExW:        HINT 'CreateWindowExW'
h_DefWindowProcW:         HINT 'DefWindowProcW'
h_RegisterClassExW:       HINT 'RegisterClassExW'
h_GetMessageW:            HINT 'GetMessageW'
h_TranslateMessage:       HINT 'TranslateMessage'
h_DispatchMessageW:       HINT 'DispatchMessageW'
h_PostQuitMessage:        HINT 'PostQuitMessage'
h_SendMessageW:           HINT 'SendMessageW'
h_SetWindowTextW:         HINT 'SetWindowTextW'
h_GetWindowTextW:         HINT 'GetWindowTextW'
h_GetWindowTextLengthW:   HINT 'GetWindowTextLengthW'
h_SetTimer:               HINT 'SetTimer'
h_KillTimer:              HINT 'KillTimer'
h_DestroyWindow:          HINT 'DestroyWindow'
h_ShowWindow:             HINT 'ShowWindow'
h_UpdateWindow:           HINT 'UpdateWindow'
h_SetFocus:               HINT 'SetFocus'
h_InvalidateRect:         HINT 'InvalidateRect'
h_LoadCursorW:            HINT 'LoadCursorW'
h_LoadIconW:              HINT 'LoadIconW'
h_PostMessageW:           HINT 'PostMessageW'
h_CommandLineToArgvW:     HINT 'CommandLineToArgvW'
h_SystemFunction036:      HINT 'SystemFunction036'
h_GetUserNameA:           HINT 'GetUserNameA'
h_NtQueryInformationProcess: HINT 'NtQueryInformationProcess'
h_GetOpenFileNameW:       HINT 'GetOpenFileNameW'
h_GetSaveFileNameW:       HINT 'GetSaveFileNameW'
h_CreateFontW:            HINT 'CreateFontW'
h_DeleteObject:           HINT 'DeleteObject'
h_CreateProcessW:         HINT 'CreateProcessW'
h_CreatePipe:             HINT 'CreatePipe'
h_SetHandleInformation:   HINT 'SetHandleInformation'
h_PeekNamedPipe:          HINT 'PeekNamedPipe'
h_GetExitCodeProcess:     HINT 'GetExitCodeProcess'
h_TerminateProcess:       HINT 'TerminateProcess'
h_GetFullPathNameW:       HINT 'GetFullPathNameW'
h_GetTempFileNameW:       HINT 'GetTempFileNameW'
h_MoveFileExW:            HINT 'MoveFileExW'
h_FlushFileBuffers:       HINT 'FlushFileBuffers'
h_WaitForSingleObject:    HINT 'WaitForSingleObject'
h_MoveWindow:             HINT 'MoveWindow'
h_GetClientRect:          HINT 'GetClientRect'
h_BeginPaint:             HINT 'BeginPaint'
h_EndPaint:               HINT 'EndPaint'
h_FillRect:               HINT 'FillRect'
h_DrawTextW:              HINT 'DrawTextW'
h_CreateAcceleratorTableW: HINT 'CreateAcceleratorTableW'
h_TranslateAcceleratorW:  HINT 'TranslateAcceleratorW'
h_DestroyAcceleratorTable: HINT 'DestroyAcceleratorTable'
h_SetProcessDPIAware:     HINT 'SetProcessDPIAware'
h_CreateFontIndirectW:    HINT 'CreateFontIndirectW'
h_CreateSolidBrush:       HINT 'CreateSolidBrush'
h_SetTextColor:           HINT 'SetTextColor'
h_SetBkColor:             HINT 'SetBkColor'
h_SetBkMode:              HINT 'SetBkMode'
h_SelectObject:           HINT 'SelectObject'
h_DwmSetWindowAttribute:  HINT 'DwmSetWindowAttribute'
h_SetWindowTheme:         HINT 'SetWindowTheme'
