; =============================================================================
; AEGIS core — память, вывод, числа, файлы
; Все WinAPI-вызовы через wcall4/5/7 (динамическое выравнивание)
; =============================================================================
HEAP_RESERVE = 32*1024*1024


; --- heap_init(rcx=hIn, rdx=hOut, r8=hErr)
heap_init:
        mov [std_in],rcx
        mov [std_out],rdx
        mov [std_err],r8
        xor eax,eax
        test rdx,rdx
        setne al
        mov [con_mode],rax
        xor ecx,ecx
        mov edx,HEAP_RESERVE
        mov r8d,0x3000          ; MEM_COMMIT|MEM_RESERVE
        mov r9d,4               ; PAGE_READWRITE
        mov rax,[VirtualAlloc]
        call wcall4
        test rax,rax
        jz .fail
        mov [h_base],rax
        mov [h_ptr],rax
        add rax,HEAP_RESERVE
        mov [h_end],rax
        ret
.fail:  xor ecx,ecx
        call [ExitProcess]

; --- halloc(rcx=размер) -> rax (bump, выровнен 16, без free)
halloc: add rcx,15
        and rcx,-16
        mov rax,[h_ptr]
        lea rdx,[rax+rcx]
        cmp rdx,[h_end]
        jae .fail
        mov [h_ptr],rdx
        ret
.fail:  mov ecx,0xE0000001
        call [ExitProcess]

; --- virt_free(rcx=ptr)
virt_free:
        xor edx,edx
        mov r8d,0x8000
        mov rax,[VirtualFree]
        call wcall4
        ret

; --- ob_grow(rcx=нужная ёмкость)
ob_grow:
        cmp rcx,[ob_cap]
        jbe .ok
        push rbx r12 r13
        mov r12,rcx
        add rcx,0xFFF
        and rcx,-0x1000
        mov rdx,rcx
        xor ecx,ecx
        mov r8d,0x3000
        mov r9d,4
        mov rax,[VirtualAlloc]
        call wcall4
        test rax,rax
        jz .fail
        mov rbx,rax             ; новый буфер
        mov rsi,[ob_buf]
        mov r13,rsi
        test rsi,rsi
        jz .fresh
        mov rdi,rbx
        mov rcx,[ob_len]
        rep movsb
        mov rcx,r13
        call virt_free
.fresh:
        mov [ob_buf],rbx
        mov [ob_cap],r12
        pop r13 r12 rbx
.ok:    ret
.fail:  mov ecx,0xE0000002
        call [ExitProcess]

; --- emit(rcx=ptr, rdx=len): захват в буфер + вывод в консоль
cld
emit:   push rbx r12
        mov rbx,rcx
        mov r12,rdx
        mov rcx,[ob_len]
        add rcx,rdx
        call ob_grow
        test r12,r12
        jz .con
        mov rdi,[ob_buf]
        add rdi,[ob_len]
        mov rsi,rbx
        mov rcx,r12
        rep movsb
        add [ob_len],r12
.con:   cmp qword [con_mode],0
        je .done
        test r12,r12
        jz .done
        mov rcx,[std_out]
        mov rdx,rbx
        mov r8,r12
        lea r9,[wr_cnt]
        mov r11d,0
        mov rax,[WriteFile]
        call wcall5
.done:  pop r12 rbx
        ret

; --- estr(rcx=z-str): вывести строку
estr:   push rbx
        mov rbx,rcx
        xor eax,eax
.l:     cmp byte [rbx+rax],0
        je .e
        inc rax
        jmp .l
.e:     mov rcx,rbx
        mov rdx,rax
        call emit
        pop rbx
        ret

; --- errstr(rcx=z-str ASCII): stderr в консоли / MessageBox в GUI
errstr: push rbx r12
        mov rbx,rcx
        xor eax,eax
.l:     cmp byte [rbx+rax],0
        je .e
        inc rax
        jmp .l
.e:     mov r12,rax
        cmp qword [con_mode],0
        je .gui
        mov rcx,[std_err]
        mov rdx,rbx
        mov r8,r12
        lea r9,[wr_cnt]
        mov r11d,0
        mov rax,[WriteFile]
        call wcall5
        jmp .done
.gui:   xor ecx,ecx
        mov rdx,rbx
        mov r8,rbx              ; заголовок = текст
        mov r9d,0x10            ; MB_ICONERROR
        mov rax,[MessageBoxA]
        call wcall4
.done:  pop r12 rbx
        ret

; --- itoa10(rax=signed, rcx=buf) -> rdx=len
itoa10: push rbx
        sub rsp,32
        xor r9d,r9d             ; флаг знака
        test rax,rax
        jns .pos
        neg rax
        mov r9d,1
        mov byte [rcx],'-'
        inc rcx
.pos:   lea r10,[rsp+31]
        mov ebx,10
.d:     xor edx,edx
        div rbx
        add dl,'0'
        dec r10
        mov [r10],dl
        test rax,rax
        jnz .d
        lea r8,[rsp+31]
        sub r8,r10              ; число цифр
        mov rsi,r10
        mov rdi,rcx
        mov rcx,r8
        rep movsb
        mov rdx,r8
        add rdx,r9
        add rsp,32
        pop rbx
        ret

; --- atoi10(rcx=ptr, rdx=len) -> rax (signed)
atoi10: xor eax,eax
        xor r8d,r8d
        mov r9d,1
        test rdx,rdx
        jz .done
.skip:  cmp r8d,edx
        jae .done
        movzx r10d,byte [rcx+r8]
        cmp r10b,' '
        je .adv
        cmp r10b,9
        je .adv
        cmp r10b,10
        je .adv
        cmp r10b,13
        je .adv
        jmp .sign
.adv:   inc r8d
        jmp .skip
.sign:  cmp r10b,'-'
        jne .chkp
        mov r9d,-1
        inc r8d
        jmp .dig
.chkp:  cmp r10b,'+'
        jne .dig
        inc r8d
.dig:   cmp r8d,edx
        jae .fin
        movzx r10d,byte [rcx+r8]
        sub r10b,'0'
        cmp r10b,9
        ja .fin
        imul rax,rax,10
        movsxd r10,r10d
        add rax,r10
        inc r8d
        jmp .dig
.fin:   imul rax,r9
.done:  ret

; --- read_file(rcx=путь W) -> rax=ptr, rdx=len (0/0 при ошибке)
read_file:
        push rbx r12 r13
        sub rsp,0x40
        mov rbx,rcx
        mov edx,0x80000000     ; GENERIC_READ
        xor r8d,r8d
        xor r9d,r9d
        mov qword [gp5],3      ; OPEN_EXISTING
        mov qword [gp6],0x80   ; FILE_ATTRIBUTE_NORMAL
        mov qword [gp7],0      ; hTemplate
        mov rax,[CreateFileW]
        call wcall7
        cmp rax,-1
        je .fail
        mov r12,rax
        mov rcx,r12
        xor edx,edx
        xor r8d,r8d
        mov r9d,2              ; FILE_END
        mov rax,[SetFilePointer]
        call wcall4
        mov r13,rax
        cmp r13,400000h        ; reject oversized/failed seeks before allocation
        ja .closefail
        mov rcx,r12
        xor edx,edx
        xor r8d,r8d
        xor r9d,r9d            ; FILE_BEGIN
        mov rax,[SetFilePointer]
        call wcall4
        lea rcx,[r13+16]
        call halloc
        mov rbx,rax
        mov rcx,r12
        mov rdx,rbx
        mov r8,r13
        lea r9,[wr_cnt]
        mov r11d,0
        mov rax,[ReadFile]
        call wcall5
        test eax,eax
        jz .closefail
        cmp r13,[wr_cnt]
        jne .closefail
        mov rcx,r12
        mov rax,[CloseHandle]
        call wcall4
        mov rax,rbx
        mov rdx,r13
        add rsp,0x40
        pop r13 r12 rbx
        ret
.closefail:
        mov rcx,r12
        mov rax,[CloseHandle]
        call wcall4
.fail:  xor eax,eax
        xor edx,edx
        add rsp,0x40
        pop r13 r12 rbx
        ret

; --- write_file(rcx=path W, rdx=ptr, r8=len) -> rax=1/0
; Write to a sibling temporary file, flush, then replace. A failed save keeps
; the old destination intact. Win32 MAX_PATH applies to this implementation.
write_file:
        push rbx r12 r13 r14
        sub rsp,860h
        mov rbx,rcx
        mov r12,rdx
        mov r13,r8
        mov edx,260
        mov r8,rsp
        lea r9,[rsp+840h]
        mov rax,[GetFullPathNameW]
        call wcall4
        test eax,eax
        jz .fail
        cmp eax,260
        jae .fail
        mov rax,[rsp+840h]
        test rax,rax
        jz .fail
        mov word [rax],0        ; keep directory, including trailing slash
        mov rcx,rsp
        lea rdx,[file_temp_prefix]
        xor r8d,r8d
        lea r9,[rsp+420h]
        mov rax,[GetTempFileNameW]
        call wcall4
        test eax,eax
        jz .fail
        lea rcx,[rsp+420h]
        mov edx,0x40000000      ; GENERIC_WRITE
        xor r8d,r8d
        xor r9d,r9d
        mov qword [gp5],2      ; CREATE_ALWAYS
        mov qword [gp6],0x80   ; FILE_ATTRIBUTE_NORMAL
        mov qword [gp7],0      ; hTemplate
        mov rax,[CreateFileW]
        call wcall7
        cmp rax,-1
        je .cleanup
        mov r14,rax
        mov rcx,r14
        mov rdx,r12
        mov r8,r13
        lea r9,[wr_cnt]
        mov r11d,0
        mov rax,[WriteFile]
        call wcall5
        test eax,eax
        jz .write_bad
        cmp r13,[wr_cnt]
        jne .write_bad
        mov rcx,r14
        mov rax,[FlushFileBuffers]
        call wcall4
        test eax,eax
        jz .write_bad
        mov r12d,1
        jmp .close
.write_bad:
        xor r12d,r12d
.close:
        mov rcx,r14
        mov rax,[CloseHandle]
        call wcall4
        test r12,r12
        jz .cleanup
        lea rcx,[rsp+420h]
        mov rdx,rbx
        mov r8d,9              ; REPLACE_EXISTING | WRITE_THROUGH
        mov rax,[MoveFileExW]
        call wcall4
        test eax,eax
        jz .cleanup
        mov eax,1
        add rsp,860h
        pop r14 r13 r12 rbx
        ret
.cleanup:
        lea rcx,[rsp+420h]
        mov rax,[DeleteFileW]
        call wcall4
.fail:  xor eax,eax
        add rsp,860h
        pop r14 r13 r12 rbx
        ret

file_temp_prefix du 'aeg',0

dval2:  push rax rcx rdx r8 r9 r10 r11 rdi rsi
        sub rsp,0x48
        lea rdi,[rsp]
        mov rax,[ccdbg]
        mov esi,16
        call dval2put
        mov ecx,-11
        mov rax,[GetStdHandle]
        call wcall4
        mov rcx,rax
        lea rdx,[rsp]
        mov r8d,16
        lea r9,[rsp+0x40]
        mov r11d,0
        mov rax,[WriteFile]
        call wcall5
        add rsp,0x48
        pop rsi rdi r11 r10 r9 r8 rdx rcx rax
        ret
dval2put:
        xor r9d,r9d
.d:     lea ecx,[rsi-1]
        sub ecx,r9d
        shl ecx,2
        mov r8,rax
        shr r8,cl
        mov edx,r8d
        and edx,15
        cmp edx,10
        jb .dig
        add edx,48
        jmp .st
.dig:   add edx,48
.st:    mov [rdi],dl
        inc rdi
        inc r9d
        cmp r9d,esi
        jb .d
        ret
