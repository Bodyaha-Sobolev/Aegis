; AEGIS Studio: native Win32/RichEdit workbench. No browser, CRT or VM.
; One document, explicit UTF-8 saves, in-memory compilation, async child output.
ID_NEW = 100
ID_OPEN = 101
ID_SAVE = 102
ID_BUILD = 103
ID_RUN = 104
ID_SAVEAS = 105
ID_EDITOR = 200

section '.text' code readable executable

ide_main:
        push rbx rbp rsi rdi r12 r13 r14 r15
        sub rsp,40h
        xor ecx,ecx
        xor edx,edx
        xor r8d,r8d
        call heap_init
        mov rax,[SetProcessDPIAware]
        call wcall4
        lea rcx,[ide_rich_dll]
        mov rax,[LoadLibraryA]
        call wcall4
        test rax,rax
        jz .fail
        xor ecx,ecx
        mov rax,[GetModuleHandleW]
        call wcall4
        mov [ide_instance],rax
        mov ecx,200004h
        call halloc
        mov [ide_widebuf],rax
        mov ecx,100004h
        call halloc
        mov [ide_utf8buf],rax
        mov ecx,00221B17h
        mov rax,[CreateSolidBrush]
        call wcall4
        mov [ide_bg],rax
        mov ecx,002C2520h
        mov rax,[CreateSolidBrush]
        call wcall4
        mov [ide_panel],rax
        mov ecx,00D1C379h
        mov rax,[CreateSolidBrush]
        call wcall4
        mov [ide_accent],rax
        lea rcx,[ide_logfont]
        mov rax,[CreateFontIndirectW]
        call wcall4
        mov [ide_font],rax
        lea rcx,[ide_codefont]
        mov rax,[CreateFontIndirectW]
        call wcall4
        mov [ide_mono],rax
        xor ecx,ecx
        mov edx,32512
        mov rax,[LoadCursorW]
        call wcall4
        mov qword [ide_wc+40],rax
        mov rax,[ide_instance]
        mov qword [ide_wc+24],rax
        mov rax,[ide_bg]
        mov qword [ide_wc+48],rax
        lea rcx,[ide_wc]
        mov rax,[RegisterClassExW]
        call wcall4
        test eax,eax
        jz .fail
        xor ecx,ecx
        lea rdx,[ide_class]
        lea r8,[ide_title]
        mov r9d,02CF0000h
        mov qword [gp5],-2147483648
        mov qword [gp6],-2147483648
        mov qword [gp7],1180
        mov qword [gp8],820
        mov qword [gp9],0
        mov qword [gp10],0
        mov rax,[ide_instance]
        mov [gp11],rax
        mov qword [gp12],0
        mov rax,[CreateWindowExW]
        call wcall12
        test rax,rax
        jz .fail
        mov [ide_hwnd],rax
        mov rcx,rax
        mov edx,20
        lea r8,[ide_true]
        mov r9d,4
        mov rax,[DwmSetWindowAttribute]
        call wcall4
        lea rcx,[ide_accels]
        mov edx,6
        mov rax,[CreateAcceleratorTableW]
        call wcall4
        mov [ide_accel],rax
        mov rcx,[ide_hwnd]
        mov edx,5
        mov rax,[ShowWindow]
        call wcall4
        mov rcx,[ide_hwnd]
        mov rax,[UpdateWindow]
        call wcall4
.loop:
        lea rcx,[ide_msg]
        xor edx,edx
        xor r8d,r8d
        xor r9d,r9d
        mov rax,[GetMessageW]
        call wcall4
        cmp eax,0
        jle .done
        mov rcx,[ide_hwnd]
        mov rdx,[ide_accel]
        lea r8,[ide_msg]
        mov rax,[TranslateAcceleratorW]
        call wcall4
        test eax,eax
        jnz .loop
        lea rcx,[ide_msg]
        mov rax,[TranslateMessage]
        call wcall4
        lea rcx,[ide_msg]
        mov rax,[DispatchMessageW]
        call wcall4
        jmp .loop
.done:
        mov rcx,[ide_accel]
        mov rax,[DestroyAcceleratorTable]
        call wcall4
        mov rcx,[ide_font]
        mov rax,[DeleteObject]
        call wcall4
        mov rcx,[ide_mono]
        mov rax,[DeleteObject]
        call wcall4
        mov rcx,[ide_bg]
        mov rax,[DeleteObject]
        call wcall4
        mov rcx,[ide_panel]
        mov rax,[DeleteObject]
        call wcall4
        mov rcx,[ide_accent]
        mov rax,[DeleteObject]
        call wcall4
        add rsp,40h
        pop r15 r14 r13 r12 rdi rsi rbp rbx
        ret
.fail:
        xor ecx,ecx
        lea rdx,[ide_init_error]
        lea r8,[ide_title]
        mov r9d,10h
        mov rax,[MessageBoxW]
        call wcall4
        jmp .done

ide_wndproc:
        push rbx rbp rsi rdi r12 r13 r14 r15
        sub rsp,40h
        mov rbx,rcx
        mov r12d,edx
        mov r13,r8
        mov r14,r9
        cmp edx,1                    ; WM_CREATE
        je .create
        cmp edx,5                    ; WM_SIZE
        je .size
        cmp edx,24h                  ; WM_GETMINMAXINFO
        je .minsize
        cmp edx,111h                 ; WM_COMMAND
        je .command
        cmp edx,113h                 ; WM_TIMER
        je .timer
        cmp edx,135h                 ; WM_CTLCOLORBTN
        je .color
        cmp edx,138h                 ; WM_CTLCOLORSTATIC
        je .color
        cmp edx,2Bh                  ; WM_DRAWITEM
        je .drawbutton
        cmp edx,10h                  ; WM_CLOSE
        je .close
        cmp edx,2                    ; WM_DESTROY
        je .destroy
.default:
        mov rcx,rbx
        mov edx,r12d
        mov r8,r13
        mov r9,r14
        mov rax,[DefWindowProcW]
        call wcall4
        jmp .finish
.create:
        mov [ide_hwnd],rbx
        call ide_controls
        xor eax,eax
        jmp .finish
.size:
        call ide_layout
        xor eax,eax
        jmp .finish
.minsize:
        mov dword [r14+24],850
        mov dword [r14+28],600
        xor eax,eax
        jmp .finish
.command:
        mov eax,r13d
        shr eax,16
        cmp eax,300h                 ; EN_CHANGE
        jne .action
        cmp r14,[ide_editor]
        jne .default
        cmp qword [ide_loading],0
        jne .zero
        mov qword [ide_dirty],1
        call ide_caption
        jmp .zero
.action:
        and r13d,0FFFFh
        cmp r13d,ID_RUN
        je .run
        cmp r13d,ID_SAVE
        je .save
        cmp r13d,ID_SAVEAS
        je .saveas
        cmp qword [ide_process],0
        jne .busy
        cmp r13d,ID_NEW
        je .new
        cmp r13d,ID_OPEN
        je .open
        cmp r13d,ID_BUILD
        je .build
        jmp .default
.new:   call ide_confirm_save
        test eax,eax
        jz .zero
        mov word [ide_path],0
        lea rdx,[ide_sample]
        call ide_set_document
        lea rcx,[ide_status_ready]
        call ide_status
        jmp .zero
.open:  call ide_confirm_save
        test eax,eax
        jz .zero
        xor ecx,ecx
        call ide_dialog
        test eax,eax
        jz .zero
        call ide_open_document
        jmp .zero
.save:  call ide_save
        jmp .zero
.saveas:
        mov ecx,1
        call ide_dialog
        test eax,eax
        jz .zero
        call ide_save_selected
        jmp .zero
.build: call ide_build
        jmp .zero
.run:
        cmp qword [ide_process],0
        je .start_run
        mov rcx,[ide_process]
        mov edx,130
        mov rax,[TerminateProcess]
        call wcall4
        jmp .zero
.start_run:
        call ide_build
        test eax,eax
        jnz .zero
        call ide_run
        jmp .zero
.busy:  lea rcx,[ide_status_busy]
        call ide_status
        jmp .zero
.timer: call ide_poll
        jmp .zero
.color:
        mov rcx,r13
        mov edx,00E8E0D8h
        mov rax,[SetTextColor]
        call wcall4
        mov rcx,r13
        mov edx,00221B17h
        mov rax,[SetBkColor]
        call wcall4
        mov rax,[ide_bg]
        jmp .finish
.drawbutton:
        mov rcx,[r14+32]             ; DRAWITEMSTRUCT.hDC (Win64)
        lea rdx,[r14+40]             ; rcItem
        mov r8,[ide_panel]
        cmp dword [r14+4],ID_RUN
        jne .fill
        mov r8,[ide_accent]
.fill:  mov rax,[FillRect]
        call wcall4
        mov rcx,[r14+32]
        mov rdx,[ide_font]
        mov rax,[SelectObject]
        call wcall4
        mov [rsp],rax
        mov rcx,[r14+32]
        mov edx,1
        mov rax,[SetBkMode]
        call wcall4
        mov rcx,[r14+32]
        mov edx,00E8E0D8h
        cmp dword [r14+4],ID_RUN
        jne .textcolor
        mov edx,00221B17h
.textcolor:
        mov rax,[SetTextColor]
        call wcall4
        mov rcx,[r14+24]             ; hwndItem
        lea rdx,[ide_buttontext]
        mov r8d,32
        mov rax,[GetWindowTextW]
        call wcall4
        mov rcx,[r14+32]
        lea rdx,[ide_buttontext]
        mov r8d,-1
        lea r9,[r14+40]
        mov r11d,25h                ; CENTER | VCENTER | SINGLELINE
        mov rax,[DrawTextW]
        call wcall5
        mov rcx,[r14+32]
        mov rdx,[rsp]
        mov rax,[SelectObject]
        call wcall4
        mov eax,1
        jmp .finish
.close:
        cmp qword [ide_process],0
        jne .busy                   ; stop through the explicit Stop button
        call ide_confirm_save
        test eax,eax
        jz .zero
        mov rcx,rbx
        mov rax,[DestroyWindow]
        call wcall4
        jmp .zero
.destroy:
        xor ecx,ecx
        mov rax,[PostQuitMessage]
        call wcall4
.zero:  xor eax,eax
.finish:
        add rsp,40h
        pop r15 r14 r13 r12 rdi rsi rbp rbx
        ret

; class/text/style/id -> child hwnd. Position is assigned by ide_layout.
ide_child:
        push rbx r12 r13 r14
        mov rbx,rcx
        mov r12,rdx
        mov r13,r8
        mov r14,r9
        xor ecx,ecx
        mov rdx,rbx
        mov r8,r12
        mov r9,r13
        mov qword [gp5],0
        mov qword [gp6],0
        mov qword [gp7],100
        mov qword [gp8],30
        mov rax,[ide_hwnd]
        mov [gp9],rax
        mov [gp10],r14
        mov rax,[ide_instance]
        mov [gp11],rax
        mov qword [gp12],0
        mov rax,[CreateWindowExW]
        call wcall12
        mov rbx,rax
        mov rcx,rax
        mov edx,30h                  ; WM_SETFONT
        mov r8,[ide_font]
        xor r9d,r9d
        mov rax,[SendMessageW]
        call wcall4
        mov rax,rbx
        pop r14 r13 r12 rbx
        ret

macro ide_label destination, text {
        lea rcx,[ide_static_class]
        lea rdx,[text]
        mov r8d,50000000h
        xor r9d,r9d
        call ide_child
        mov [destination],rax
}
macro ide_button destination, text, id {
        lea rcx,[ide_button_class]
        lea rdx,[text]
        mov r8d,5001000Bh
        mov r9d,id
        call ide_child
        mov [destination],rax
}

ide_controls:
        ide_label ide_brand, ide_brand_text
        ide_label ide_tagline, ide_tagline_text
        ide_label ide_explorer, ide_explorer_text
        ide_label ide_language, ide_language_text
        ide_label ide_doclabel, ide_untitled
        ide_label ide_outputlabel, ide_output_text
        ide_label ide_statusbar, ide_status_ready
        ide_button ide_newbutton, ide_new_text, ID_NEW
        ide_button ide_openbutton, ide_open_text, ID_OPEN
        ide_button ide_savebutton, ide_save_text, ID_SAVE
        ide_button ide_buildbutton, ide_build_text, ID_BUILD
        ide_button ide_runbutton, ide_run_text, ID_RUN
        lea rcx,[ide_rich_class]
        lea rdx,[ide_empty]
        mov r8d,503111C4h
        mov r9d,ID_EDITOR
        call ide_child
        mov [ide_editor],rax
        mov rcx,rax
        mov edx,459h                 ; EM_SETTEXTMODE (empty control only)
        mov r8d,9                    ; TM_PLAINTEXT | TM_MULTILEVELUNDO
        xor r9d,r9d
        mov rax,[SendMessageW]
        call wcall4
        mov rax,[ide_editor]
        mov rcx,rax
        call ide_style_editor
        lea rcx,[ide_rich_class]
        lea rdx,[ide_welcome]
        mov r8d,502008C4h
        mov r9d,201
        call ide_child
        mov [ide_output],rax
        mov rcx,rax
        call ide_style_editor
        mov rcx,[ide_editor]
        mov edx,435h                 ; EM_EXLIMITTEXT
        xor r8d,r8d
        mov r9d,0FFFFFh
        mov rax,[SendMessageW]
        call wcall4
        mov rcx,[ide_editor]
        mov edx,445h                 ; EM_SETEVENTMASK
        xor r8d,r8d
        mov r9d,1                    ; ENM_CHANGE
        mov rax,[SendMessageW]
        call wcall4
        lea rdx,[ide_sample]
        call ide_set_document
        ret

ide_style_editor:
        push rbx
        mov rbx,rcx
        lea rdx,[ide_dark_theme]
        xor r8d,r8d
        mov rax,[SetWindowTheme]
        call wcall4
        mov rcx,rbx
        mov edx,443h                 ; EM_SETBKGNDCOLOR
        xor r8d,r8d
        mov r9d,00221B17h
        mov rax,[SendMessageW]
        call wcall4
        mov rcx,rbx
        mov edx,30h
        mov r8,[ide_mono]
        xor r9d,r9d
        mov rax,[SendMessageW]
        call wcall4
        mov rcx,rbx
        mov edx,444h                 ; EM_SETCHARFORMAT
        mov r8d,4                    ; SCF_ALL
        lea r9,[ide_charformat]
        mov rax,[SendMessageW]
        call wcall4
        mov rcx,rbx
        mov edx,0D3h                 ; EM_SETMARGINS
        mov r8d,3
        mov r9d,00100010h
        mov rax,[SendMessageW]
        call wcall4
        pop rbx
        ret

; rcx=hwnd, edx=x, r8d=y, r9d=width, eax=height.
ide_move:
        mov [gp5],rax
        mov qword [gp6],1
        mov qword [gp7],0
        mov rax,[MoveWindow]
        call wcall7
        ret

ide_layout:
        push rbx r12
        mov rcx,[ide_hwnd]
        lea rdx,[ide_client]
        mov rax,[GetClientRect]
        call wcall4
        mov ebx,[ide_client+8]
        mov r12d,[ide_client+12]
        mov rcx,[ide_brand]
        mov edx,24
        mov r8d,22
        mov r9d,160
        mov eax,26
        call ide_move
        mov rcx,[ide_tagline]
        mov edx,216
        mov r8d,23
        lea r9d,[rbx-704]
        mov eax,24
        call ide_move
        mov rcx,[ide_explorer]
        mov edx,24
        mov r8d,92
        mov r9d,160
        mov eax,140
        call ide_move
        mov rcx,[ide_language]
        mov edx,24
        mov r8d,270
        mov r9d,160
        mov eax,250
        call ide_move
        mov rcx,[ide_doclabel]
        mov edx,216
        mov r8d,77
        lea r9d,[rbx-240]
        mov eax,26
        call ide_move
        mov rcx,[ide_editor]
        mov edx,216
        mov r8d,108
        lea r9d,[rbx-240]
        lea eax,[r12-370]
        call ide_move
        mov rcx,[ide_outputlabel]
        mov edx,216
        lea r8d,[r12-247]
        lea r9d,[rbx-240]
        mov eax,24
        call ide_move
        mov rcx,[ide_output]
        mov edx,216
        lea r8d,[r12-216]
        lea r9d,[rbx-240]
        mov eax,164
        call ide_move
        mov rcx,[ide_statusbar]
        mov edx,24
        lea r8d,[r12-30]
        lea r9d,[rbx-48]
        mov eax,24
        call ide_move
        ; Compact toolbar remains usable at the minimum window width.
        mov rcx,[ide_newbutton]
        lea edx,[rbx-454]
        mov r8d,16
        mov r9d,68
        mov eax,34
        call ide_move
        mov rcx,[ide_openbutton]
        lea edx,[rbx-378]
        mov r8d,16
        mov r9d,68
        mov eax,34
        call ide_move
        mov rcx,[ide_savebutton]
        lea edx,[rbx-302]
        mov r8d,16
        mov r9d,68
        mov eax,34
        call ide_move
        mov rcx,[ide_buildbutton]
        lea edx,[rbx-226]
        mov r8d,16
        mov r9d,94
        mov eax,34
        call ide_move
        mov rcx,[ide_runbutton]
        lea edx,[rbx-124]
        mov r8d,16
        mov r9d,100
        mov eax,34
        call ide_move
        pop r12 rbx
        ret

ide_status:                      ; rcx=text
        mov rdx,rcx
        mov rcx,[ide_statusbar]
        mov rax,[SetWindowTextW]
        call wcall4
        ret

ide_caption:
        mov rcx,[ide_hwnd]
        lea rdx,[ide_title]
        cmp qword [ide_dirty],0
        je .set
        lea rdx,[ide_title_dirty]
.set:   mov rax,[SetWindowTextW]
        call wcall4
        mov rcx,[ide_doclabel]
        lea rdx,[ide_untitled]
        cmp word [ide_path],0
        je .document
        lea rdx,[ide_path]
.document:
        mov rax,[SetWindowTextW]
        call wcall4
        ret

ide_set_document:                ; rdx=UTF-16 NUL terminated
        mov qword [ide_loading],1
        mov rcx,[ide_editor]
        mov rax,[SetWindowTextW]
        call wcall4
        mov qword [ide_loading],0
        mov qword [ide_dirty],0
        call ide_caption
        mov rcx,[ide_editor]
        mov rax,[SetFocus]
        call wcall4
        ret

; Dialog edits a separate buffer. Cancelling never changes the document path.
ide_dialog:                      ; ecx=0 open, 1 save
        push rbx rsi rdi
        mov ebx,ecx
        lea rsi,[ide_path]
        lea rdi,[ide_dialogpath]
        mov ecx,32768
        rep movsw
        mov rax,[ide_hwnd]
        mov qword [ide_ofn+8],rax
        mov dword [ide_ofn+96],02081808h
        test ebx,ebx
        jz .open
        mov dword [ide_ofn+96],0208000Ah
        mov rax,[GetSaveFileNameW]
        jmp .call
.open:  mov rax,[GetOpenFileNameW]
.call:  lea rcx,[ide_ofn]
        call wcall4
        pop rdi rsi rbx
        ret

ide_commit_path:
        push rsi rdi
        lea rsi,[ide_dialogpath]
        lea rdi,[ide_path]
        mov ecx,32768
        rep movsw
        pop rdi rsi
        ret

ide_open_document:
        push rbx r12
        mov r12,[h_ptr]
        lea rcx,[ide_dialogpath]
        call read_file
        test rax,rax
        jz .error
        cmp rdx,100000h
        ja .error
        test rdx,rdx
        jz .empty
        xor ecx,ecx
.check_nul:
        cmp byte [rax+rcx],0
        je .error
        inc rcx
        cmp rcx,rdx
        jb .check_nul
        mov r9,rdx
        mov r8,rax
        mov ecx,65001
        mov edx,8                    ; MB_ERR_INVALID_CHARS
        mov rax,[ide_widebuf]
        mov [gp5],rax
        mov qword [gp6],100000h
        mov rax,[MultiByteToWideChar]
        call wcall7
        test eax,eax
        jz .error
        jmp .converted
.empty: xor eax,eax
.converted:
        mov rdx,[ide_widebuf]
        mov word [rdx+rax*2],0
        call ide_commit_path
        mov rdx,[ide_widebuf]
        call ide_set_document
        lea rcx,[ide_status_open]
        call ide_status
        jmp .done
.error: lea rcx,[ide_status_read_error]
        call ide_status
.done:  mov [h_ptr],r12
        pop r12 rbx
        ret

ide_source_utf8:                 ; eax=length, zero/error=-1
        mov rcx,[ide_editor]
        mov rdx,[ide_widebuf]
        mov r8d,100001h
        mov rax,[GetWindowTextW]
        call wcall4
        test eax,eax
        jz .done
        mov r9d,eax
        mov ecx,65001
        mov edx,80h                  ; WC_ERR_INVALID_CHARS
        mov r8,[ide_widebuf]
        mov rax,[ide_utf8buf]
        mov [gp5],rax
        mov qword [gp6],100000h
        mov qword [gp7],0
        mov qword [gp8],0
        mov rax,[WideCharToMultiByte]
        call wcall8
        test eax,eax
        jnz .done
        mov eax,-1
.done:  ret

ide_save:
        push rbx
        cmp word [ide_path],0
        jne .write
        mov ecx,1
        call ide_dialog
        test eax,eax
        jz .cancel
        mov ebx,1
        jmp .convert
.write: xor ebx,ebx
.convert:
        call ide_source_utf8
        cmp eax,-1
        je .error
        mov r8d,eax
        lea rcx,[ide_path]
        test ebx,ebx
        jz .path
        lea rcx,[ide_dialogpath]
.path:  mov rdx,[ide_utf8buf]
        call write_file
        test eax,eax
        jz .error
        test ebx,ebx
        jz .saved
        call ide_commit_path
.saved:
        mov qword [ide_dirty],0
        call ide_caption
        lea rcx,[ide_status_saved]
        call ide_status
        mov eax,1
        pop rbx
        ret
.error: lea rcx,[ide_status_write_error]
        call ide_status
.cancel:
        xor eax,eax
        pop rbx
        ret

ide_save_selected:
        push rbx
        mov ebx,1
        jmp ide_save.convert

ide_confirm_save:
        cmp qword [ide_dirty],0
        je .yes
        mov rcx,[ide_hwnd]
        lea rdx,[ide_save_question]
        lea r8,[ide_title]
        mov r9d,33h                  ; YESNOCANCEL | ICONWARNING
        mov rax,[MessageBoxW]
        call wcall4
        cmp eax,6
        je ide_save
        cmp eax,7
        je .yes
        xor eax,eax
        ret
.yes:   mov eax,1
        ret

ide_build:
        push rbx rsi rdi
        cmp word [ide_path],0
        jne .source
        call ide_save
        test eax,eax
        jz .failed
.source:
        call ide_source_utf8
        cmp eax,-1
        je .conversion_error
        mov ebx,eax
        lea rsi,[ide_path]
        lea rdi,[ide_exepath]
        xor ecx,ecx
.copy:  cmp ecx,32760
        jae .conversion_error
        lodsw
        test ax,ax
        jz .suffix
        stosw
        inc ecx
        jmp .copy
.suffix:
        mov word [rdi],'.'
        mov word [rdi+2],'e'
        mov word [rdi+4],'x'
        mov word [rdi+6],'e'
        mov word [rdi+8],0
        mov qword [ob_len],0
        mov rcx,[ide_utf8buf]
        mov edx,ebx
        lea r8,[ide_exepath]
        call agc_compile
        mov ebx,eax
        test eax,eax
        jnz .compile_error
        lea rcx,[m_b1]
        call estr
        lea rcx,[ide_status_built]
        call ide_status
        jmp .log
.compile_error:
        lea rcx,[ide_status_compile_error]
        call ide_status
.log:   call ide_show_log
        mov eax,ebx
        jmp .done
.conversion_error:
        lea rcx,[ide_status_write_error]
        call ide_status
.failed:
        mov eax,1
.done:  pop rdi rsi rbx
        ret

ide_show_log:
        cmp qword [ob_len],0
        je .empty
        mov ecx,65001
        xor edx,edx
        mov r8,[ob_buf]
        mov r9,[ob_len]
        mov rax,[ide_widebuf]
        mov [gp5],rax
        mov qword [gp6],100000h
        mov rax,[MultiByteToWideChar]
        call wcall7
        mov rdx,[ide_widebuf]
        mov word [rdx+rax*2],0
        jmp .set
.empty: lea rdx,[ide_empty]
.set:   mov rcx,[ide_output]
        mov rax,[SetWindowTextW]
        call wcall4
        mov rcx,[ide_output]
        mov edx,0B1h                 ; EM_SETSEL(-1,-1)
        mov r8,-1
        mov r9,-1
        mov rax,[SendMessageW]
        call wcall4
        mov rcx,[ide_output]
        mov edx,0B7h                 ; EM_SCROLLCARET
        xor r8d,r8d
        xor r9d,r9d
        mov rax,[SendMessageW]
        call wcall4
        ret

; Child process runs without blocking the UI. Only its own output is captured.
ide_run:
        lea rcx,[ide_pipe]
        lea rdx,[ide_pipewrite]
        lea r8,[ide_security]
        xor r9d,r9d
        mov rax,[CreatePipe]
        call wcall4
        test eax,eax
        jz .error
        mov rcx,[ide_pipe]
        mov edx,1
        xor r8d,r8d
        mov rax,[SetHandleInformation]
        call wcall4
        test eax,eax
        jz .pipe_error
        mov rax,[ide_pipewrite]
        mov qword [ide_startup+88],rax
        mov qword [ide_startup+96],rax
        lea rcx,[ide_exepath]
        xor edx,edx
        xor r8d,r8d
        xor r9d,r9d
        mov qword [gp5],1            ; inherit stdout/stderr pipe
        mov qword [gp6],08000000h    ; CREATE_NO_WINDOW
        mov qword [gp7],0
        mov qword [gp8],0
        lea rax,[ide_startup]
        mov [gp9],rax
        lea rax,[ide_processinfo]
        mov [gp10],rax
        mov qword [gp11],0
        mov qword [gp12],0
        mov rax,[CreateProcessW]
        call wcall12
        test eax,eax
        jz .pipe_error
        mov rax,[ide_processinfo]
        mov [ide_process],rax
        mov rcx,[ide_processinfo+8]
        mov rax,[CloseHandle]
        call wcall4
        mov rcx,[ide_pipewrite]
        mov rax,[CloseHandle]
        call wcall4
        mov qword [ide_pipewrite],0
        mov qword [ob_len],0
        call ide_show_log
        mov rcx,[ide_runbutton]
        lea rdx,[ide_stop_text]
        mov rax,[SetWindowTextW]
        call wcall4
        lea rcx,[ide_status_running]
        call ide_status
        mov rcx,[ide_hwnd]
        mov edx,1
        mov r8d,100
        xor r9d,r9d
        mov rax,[SetTimer]
        call wcall4
        test rax,rax
        jnz .done
        ; No polling timer means no useful Run session; release the child.
        mov rcx,[ide_process]
        mov edx,130
        mov rax,[TerminateProcess]
        call wcall4
        call ide_end_process
        jmp .error
.pipe_error:
        mov rcx,[ide_pipe]
        mov rax,[CloseHandle]
        call wcall4
        mov rcx,[ide_pipewrite]
        mov rax,[CloseHandle]
        call wcall4
        mov qword [ide_pipe],0
        mov qword [ide_pipewrite],0
.error: lea rcx,[ide_status_run_error]
        call ide_status
.done:  ret

ide_poll:
        push rbx r12
        cmp qword [ide_process],0
        je .done
        xor ebx,ebx
        xor r12d,r12d
.read:
        mov rcx,[ide_pipe]
        xor edx,edx
        xor r8d,r8d
        xor r9d,r9d
        lea rax,[ide_available]
        mov [gp5],rax
        mov qword [gp6],0
        mov qword [gp7],0
        mov rax,[PeekNamedPipe]
        call wcall7
        test eax,eax
        jz .check_exit
        mov eax,[ide_available]
        test eax,eax
        jz .check_exit
        cmp eax,4096
        jbe .chunk
        mov eax,4096
.chunk:
        mov r8d,eax
        mov rcx,[ide_pipe]
        lea rdx,[ide_pipebuf]
        lea r9,[ide_readcount]
        mov r11d,0
        mov rax,[ReadFile]
        call wcall5
        test eax,eax
        jz .check_exit
        mov edx,[ide_readcount]
        test edx,edx
        jz .check_exit
        add ebx,edx
        cmp qword [ob_len],100000h
        jae .budget
        mov rax,100000h
        sub rax,[ob_len]
        cmp rdx,rax
        jbe .append
        mov rdx,rax
.append:
        lea rcx,[ide_pipebuf]
        call emit
.budget:
        cmp ebx,10000h
        jb .read
        jmp .refresh
.check_exit:
        test r12d,r12d
        jnz .completed
        mov rcx,[ide_process]
        xor edx,edx
        mov rax,[WaitForSingleObject]
        call wcall4
        test eax,eax
        jnz .refresh
        mov rcx,[ide_process]
        lea rdx,[ide_exitcode]
        mov rax,[GetExitCodeProcess]
        call wcall4
        mov r12d,1
        jmp .read                   ; drain bytes written just before exit
.completed:
        lea rcx,[ide_exit_prefix]
        call estr
        mov eax,[ide_exitcode]
        call agc_print_number
        lea rcx,[ide_exit_suffix]
        call estr
        call ide_end_process
        lea rcx,[ide_status_finished]
        call ide_status
.refresh:
        call ide_show_log
.done:  pop r12 rbx
        ret

ide_end_process:
        mov rcx,[ide_process]
        mov rax,[CloseHandle]
        call wcall4
        mov rcx,[ide_pipe]
        mov rax,[CloseHandle]
        call wcall4
        mov qword [ide_process],0
        mov qword [ide_pipe],0
        mov rcx,[ide_hwnd]
        mov edx,1
        mov rax,[KillTimer]
        call wcall4
        mov rcx,[ide_runbutton]
        lea rdx,[ide_run_text]
        mov rax,[SetWindowTextW]
        call wcall4
        ret

section '.data' data readable writeable
align 8
gp8 dq 0
gp9 dq 0
gp10 dq 0
gp11 dq 0
gp12 dq 0
ide_instance dq 0
ide_hwnd dq 0
ide_editor dq 0
ide_output dq 0
ide_brand dq 0
ide_tagline dq 0
ide_explorer dq 0
ide_language dq 0
ide_doclabel dq 0
ide_outputlabel dq 0
ide_statusbar dq 0
ide_newbutton dq 0
ide_openbutton dq 0
ide_savebutton dq 0
ide_buildbutton dq 0
ide_runbutton dq 0
ide_font dq 0
ide_mono dq 0
ide_bg dq 0
ide_panel dq 0
ide_accent dq 0
ide_accel dq 0
ide_widebuf dq 0
ide_utf8buf dq 0
ide_dirty dq 0
ide_loading dq 0
ide_process dq 0
ide_pipe dq 0
ide_pipewrite dq 0
ide_exitcode dd 0
ide_available dd 0
ide_readcount dd 0
ide_true dd 1
ide_client rd 4
ide_msg rb 48
ide_buttontext rw 32
ide_pipebuf rb 4096
ide_processinfo rq 3
ide_security dd 24,0
        dq 0
        dd 1,0
ide_startup dd 104,0
        dq 0,0,0
        dd 0,0,0,0,0,0,0,101h
        dw 0,0
        dd 0
        dq 0,0,0,0
ide_wc dd 80,3
        dq ide_wndproc
        dd 0,0
        dq 0,0,0,0,0,ide_class,0
ide_logfont dd -16,0,0,0,400
        db 0,0,0,1,0,0,5,0
        du 'Segoe UI',0
        dw 23 dup(0)
ide_codefont dd -18,0,0,0,400
        db 0,0,0,1,0,0,5,31h
        du 'Consolas',0
        dw 23 dup(0)
ide_charformat dd 116,40000000h,0,0,0,00E8E0D8h
        db 1,0
        dw 32 dup(0)
        dw 0
        db 24 dup(0)
ide_accels:
        db 9,0
        dw 'N',ID_NEW
        db 9,0
        dw 'O',ID_OPEN
        db 9,0
        dw 'S',ID_SAVE
        db 13,0
        dw 'S',ID_SAVEAS
        db 1,0
        dw 74h,ID_RUN
        db 1,0
        dw 75h,ID_BUILD
ide_ofn dd 152,0
        dq 0,0,ide_filter,0
        dd 0,1
        dq ide_dialogpath
        dd 32768,0
        dq 0
        dd 0,0
        dq 0,0
        dd 0
        dw 0,0
        dq ide_extension,0,0,0,0
        dd 0,0
ide_class du 'AEGISStudioWindow',0
ide_static_class du 'STATIC',0
ide_button_class du 'BUTTON',0
ide_rich_class du 'RICHEDIT50W',0
ide_rich_dll db 'Msftedit.dll',0
ide_dark_theme du 'DarkMode_Explorer',0
ide_title du 'AEGIS Studio',0
ide_title_dirty du '* AEGIS Studio',0
ide_brand_text du 'A E G I S',0
ide_tagline_text du 'STUDIO  /  NATIVE x64',0
ide_explorer_text du 'WORKSPACE',13,10,13,10,'Single document',13,10,'UTF-8 source',13,10,13,10,'Builds beside source',0
ide_language_text du 'QUICK REFERENCE',13,10,13,10,'fn main() { ... }',13,10,'let x = 42',13,10,'if (x > 0) { ... }',13,10,'while (x < 10) { ... }',13,10,'printi(x)',13,10,'return 0',13,10,13,10,'Ctrl+O  Open',13,10,'Ctrl+S  Save',13,10,'F6  Build',13,10,'F5  Build and run',0
ide_untitled du 'untitled.ae  /  UTF-8',0
ide_output_text du 'OUTPUT  /  BUILD & RUN',0
ide_welcome du 'Ready. Write AEGIS source, then build a native executable.',13,10,'F6 builds. F5 builds and captures program output here.',0
ide_new_text du 'New',0
ide_open_text du 'Open',0
ide_save_text du 'Save',0
ide_build_text du 'Build  F6',0
ide_run_text du 'Run  F5',0
ide_stop_text du 'Stop',0
ide_empty du 0
ide_status_ready du 'READY    /    AEGIS native compiler    /    UTF-8    /    x86-64',0
ide_status_open du 'OPENED    /    UTF-8 source loaded',0
ide_status_saved du 'SAVED    /    UTF-8 source written',0
ide_status_built du 'BUILD SUCCEEDED    /    Native executable: <source>.exe',0
ide_status_compile_error du 'BUILD FAILED    /    See line and column in Output',0
ide_status_read_error du 'OPEN FAILED    /    Expected UTF-8 text, at most 1 MiB',0
ide_status_write_error du 'SAVE FAILED    /    Check the path, permissions and source size',0
ide_status_run_error du 'RUN FAILED    /    Could not start or monitor the executable',0
ide_status_running du 'RUNNING    /    Output is captured below    /    Press Stop to end the program',0
ide_status_finished du 'FINISHED    /    Exit code is shown in Output    /    Output limit: 1 MiB',0
ide_status_busy du 'PROGRAM RUNNING    /    Press Stop before building, opening or closing',0
ide_save_question du 'Save changes to the current document?',0
ide_init_error du 'Could not initialize the native editor.',0
ide_exit_prefix db 10,'[Process exited with code ',0
ide_exit_suffix db ']',10,0
ide_filter du 'AEGIS source (*.ae)',0,'*.ae',0,'All files (*.*)',0,'*.*',0,0
ide_extension du 'ae',0
ide_sample du '// AEGIS compiles directly to native x86-64.',13,10
        du '// F6: build     F5: build and run',13,10,13,10
        du 'fn factorial(n) {',13,10
        du '    if (n <= 1) {',13,10
        du '        return 1;',13,10
        du '    }',13,10
        du '    return n * factorial(n - 1);',13,10
        du '}',13,10,13,10
        du 'fn main() {',13,10
        du '    let value = factorial(6);',13,10
        du '    printi(value);',13,10
        du '    return 0;',13,10
        du '}',13,10,0
ide_path rw 32768
ide_dialogpath rw 32768
ide_exepath rw 32768
