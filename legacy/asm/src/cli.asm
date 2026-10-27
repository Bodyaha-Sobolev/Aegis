; =============================================================================
; AEGIS CLI: enc / dec / test / build
; =============================================================================
section '.text' code readable executable

getstd: mov rax,[GetStdHandle]
        call wcall4
        ret

phex:   push rbx
        sub rsp,32
        mov ebx,ecx
        xor r9d,r9d
.d:     lea ecx,[ebx-1]
        sub ecx,r9d
        shl ecx,2
        mov r8,rax
        shr r8,cl
        mov edx,r8d
        and edx,15
        cmp edx,10
        jb .dig
        add edx,87
        jmp .st
.dig:   add edx,48
.st:    mov [rsp+r9],dl
        inc r9d
        cmp r9d,ebx
        jb .d
        mov rcx,rsp
        mov rdx,r9
        call emit
        add rsp,32
        pop rbx
        ret

weq:    xor eax,eax
.l:     movzx r8d,word [rcx+rax*2]
        movzx r9d,byte [rdx+rax]
        cmp r8d,r9d
        jne .ne
        test r8d,r8d
        jz .eq
        inc eax
        jmp .l
.eq:    test r9d,r9d
        jnz .ne
        cmp eax,eax
        ret
.ne:    or eax,1
        ret

watoi:  xor eax,eax
        xor r8d,r8d
        mov r9d,10
        cmp word [rcx],'0'
        jne .lp
        cmp word [rcx+2],'x'
        je .hex
        cmp word [rcx+2],'X'
        jne .lp
.hex:
        add r8d,2
        mov r9d,16
.lp:    movzx r10d,word [rcx+r8*2]
        test r10d,r10d
        jz .done
        mov r11d,r10d
        sub r10d,'0'
        cmp r10d,9
        jbe .checkbase
        or r11d,20h
        sub r11d,'a'
        cmp r11d,5
        ja .bad
        add r11d,10
        mov r10d,r11d
.checkbase:
        cmp r10d,r9d
        jae .bad
.acc:   imul eax,r9d
        add eax,r10d
        cmp eax,0FFFh
        ja .bad
        inc r8d
        jmp .lp
.done:  test r8d,r8d
        jz .bad
        cmp r9d,16
        jne .ok
        cmp r8d,2
        je .bad
.ok:    clc
        ret
.bad:   stc
        ret

cli_main:
        push rbx r12 r13 r14 r15
        mov rbx,rsi
        xor r13d,r13d
        mov r14d,-1
        mov qword [g_pos],0
        mov qword [g_pos+8],0
        mov r12d,2
        mov ecx,-11
        call getstd
        mov [g_out],rax
        mov ecx,-10
        call getstd
        mov [g_in],rax
        mov ecx,-12
        call getstd
        mov [g_err],rax
        test rax,rax
        jnz .haveh
        mov ecx,-1
        mov rax,[AttachConsole]
        call wcall4
        mov ecx,-11
        call getstd
        mov [g_out],rax
        mov ecx,-10
        call getstd
        mov [g_in],rax
        mov ecx,-12
        call getstd
        mov [g_err],rax
.haveh: mov rcx,[g_in]
        mov rdx,[g_out]
        mov r8,[g_err]
        call heap_init
.args:  mov eax,dword [g_argc]
        cmp r12d,eax
        jae .disp
        mov rcx,[rbx+r12*8]
        cmp word [rcx],'-'
        jne .pos
        cmp word [rcx+4],0
        jne .usage
        movzx eax,word [rcx+2]
        cmp eax,'k'
        je .takekey
        cmp eax,'c'
        je .takecode
        jmp .usage
.takekey:
        inc r12d
        mov eax,dword [g_argc]
        cmp r12d,eax
        jae .usage
        mov rax,[rbx+r12*8]
        mov r13,rax
        inc r12d
        jmp .args
.takecode:
        inc r12d
        mov eax,dword [g_argc]
        cmp r12d,eax
        jae .usage
        mov rcx,[rbx+r12*8]
        call watoi
        jc .usage
        mov r14d,eax
        inc r12d
        jmp .args
.pos:   cmp qword [g_pos],0
        je .p0
        cmp qword [g_pos+8],0
        je .p1
        jmp .usage
.p0:    mov [g_pos],rcx
        inc r12d
        jmp .args
.p1:    mov [g_pos+8],rcx
        inc r12d
        jmp .args
.disp:  mov rcx,[rbx+8]
        lea rdx,[cli_cmd_test]
        call weq
        je .test
        mov rcx,[rbx+8]
        lea rdx,[cli_cmd_enc]
        call weq
        je .enc
        mov rcx,[rbx+8]
        lea rdx,[cli_cmd_dec]
        call weq
        je .dec
        mov rcx,[rbx+8]
        lea rdx,[cli_cmd_build]
        call weq
        je .build
        mov rcx,[rbx+8]
        lea rdx,[cli_cmd_render]
        call weq
        je .unsupported
        mov rcx,[rbx+8]
        lea rdx,[cli_cmd_unrender]
        call weq
        je .unsupported
.usage:  lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.test:  call selftest
        jmp .fin
.enc:   call cmd_enc
        jmp .fin
.dec:   call cmd_dec
        jmp .fin
.build: call cmd_build
        jmp .fin
.unsupported:
        lea rcx,[m_unavailable]
        call estr
        mov eax,2
        jmp .fin
.fin:   pop r15 r14 r13 r12 rbx
        ret

; --- cmd_render: .aeg -> cursed text (utf16)
; кадр 0x40: [0]=in [8]=inlen [0x10]=key [0x18]=keylen [0x20]=key32
cmd_render:
        push rbp
        sub rsp,0x40
        cmp qword [g_pos],0
        je .usage
        cmp qword [g_pos+8],0
        je .usage
        test r13,r13
        jz .nokey
        mov rcx,[g_pos]
        call read_file
        test rax,rax
        jz .noin
        mov [rsp],rax
        mov [rsp+8],rdx
        mov rcx,r13
        call read_file
        test rax,rax
        jz .nokey
        mov [rsp+0x10],rax
        mov [rsp+0x18],rdx
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        mov r8d,32
        mov r9d,0
        lea rax,[rsp+0x20]
        push rax
        sub rsp,0x20
        call codec_key
        add rsp,0x20
        pop rax
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        lea r8,[rsp+0x20]
        call cursed_render
        ; запись utf16: BOM уже внутри
        mov r12,rax
        mov r13,rdx
        mov rcx,[g_pos+8]
        mov rdx,r12
        mov r8,r13
        shl r8,1
        call write_file
        lea rcx,[m_rend1]
        call estr
        xor eax,eax
        jmp .fin
.usage:  lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.nokey:  lea rcx,[m_nokey]
        call estr
        mov eax,2
        jmp .fin
.noin:   lea rcx,[m_noin]
        call estr
        mov eax,2
        jmp .fin
.fin:   add rsp,0x40
        pop rbp
        ret

; --- cmd_derender: cursed text -> .aeg
cmd_derender:
        push rbp
        sub rsp,0x40
        cmp qword [g_pos],0
        je .usage
        cmp qword [g_pos+8],0
        je .usage
        test r13,r13
        jz .nokey
        mov rcx,[g_pos]
        call read_file
        test rax,rax
        jz .noin
        mov [rsp],rax
        mov [rsp+8],rdx
        mov rcx,r13
        call read_file
        test rax,rax
        jz .nokey
        mov [rsp+0x10],rax
        mov [rsp+0x18],rdx
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        mov r8d,32
        mov r9d,0
        lea rax,[rsp+0x20]
        push rax
        sub rsp,0x20
        call codec_key
        add rsp,0x20
        pop rax
        mov rax,0
        test rax,rax
        jz .badenc
        mov r13,rdx
        mov rcx,[g_pos+8]
        mov rdx,rax
        mov r8,r13
        call write_file
        lea rcx,[m_dere1]
        call estr
        xor eax,eax
        jmp .fin
.badenc: lea rcx,[m_badenc]
        call estr
        mov eax,2
        jmp .fin
.usage:  lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.nokey:  lea rcx,[m_nokey]
        call estr
        mov eax,2
        jmp .fin
.noin:   lea rcx,[m_noin]
        call estr
        mov eax,2
        jmp .fin
.fin:   add rsp,0x40
        pop rbp
        ret

; --- cmd_enc
cmd_enc:
        push rbp
        sub rsp,0x60
        cmp qword [g_pos],0
        je .usage
        cmp qword [g_pos+8],0
        je .usage
        test r13,r13
        jz .nokey
        cmp r14d,-1
        jne .have
        lea rcx,[rsp+0x38]
        mov edx,2
        call fill_rand
        movzx eax,word [rsp+0x38]
        and eax,0xFFF
        mov r14d,eax
.have:  mov rcx,[g_pos]
        call read_file
        test rax,rax
        jz .noin
        mov [rsp],rax
        mov [rsp+8],rdx
        mov rcx,r13
        call read_file
        test rax,rax
        jz .nokey
        mov [rsp+0x10],rax
        mov [rsp+0x18],rdx
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        mov r8,[rsp+8]
        mov r9d,r14d
        and r9d,0xFFF
        lea rax,[rsp+0x28]
        push rax
        sub rsp,0x20
        call codec_key
        add rsp,0x20
        pop rax
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        lea r8,[rsp+0x28]
        call codec_encode
        mov [rsp+0x48],rax
        mov [rsp+0x50],rdx
        lea rcx,[rsp+0x28]
        mov edx,32
        call wipe
        mov rcx,[g_pos+8]
        mov rdx,[rsp+0x48]
        mov r8,[rsp+0x50]
        call write_file
        test rax,rax
        jz .noout
        lea rcx,[m_enc1]
        call estr
        mov rax,[rsp+8]
        sub rsp,32
        mov rcx,rsp
        call itoa10
        mov rcx,rsp
        call emit
        add rsp,32
        lea rcx,[m_enc2]
        call estr
        mov eax,r14d
        and eax,0xFFF
        mov ecx,3
        call phex
        lea rcx,[m_enc3]
        call estr
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        call wipe
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        call wipe
        xor eax,eax
        jmp .fin
.usage:  lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.nokey:  lea rcx,[m_nokey]
        call estr
        mov eax,2
        jmp .fin
.noin:   lea rcx,[m_noin]
        call estr
        mov eax,2
        jmp .fin
.noout:  lea rcx,[m_noout]
        call estr
        mov eax,2
        jmp .fin
.fin:   add rsp,0x60
        pop rbp
        ret

; --- cmd_dec
cmd_dec:
        push rbp
        sub rsp,0x60
        cmp qword [g_pos],0
        je .usage
        cmp qword [g_pos+8],0
        je .usage
        test r13,r13
        jz .nokey
        cmp r14d,-1
        je .nocode
        mov rcx,[g_pos]
        call read_file
        test rax,rax
        jz .noin
        mov [rsp],rax
        mov [rsp+8],rdx
        cmp rdx,44
        jb .badenc
        mov rcx,r13
        call read_file
        test rax,rax
        jz .nokey
        mov [rsp+0x10],rax
        mov [rsp+0x18],rdx
        mov rax,[rsp]
        mov r8d,[rax+8]
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        mov r9d,r14d
        and r9d,0xFFF
        lea rax,[rsp+0x28]
        push rax
        sub rsp,0x20
        call codec_key
        add rsp,0x20
        pop rax
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        lea r8,[rsp+0x28]
        call codec_decode
        mov [rsp+0x48],rax
        mov [rsp+0x50],rdx
        lea rcx,[rsp+0x28]
        mov edx,32
        call wipe
        cmp qword [rsp+0x48],0
        je .badenc
        mov rcx,[g_pos+8]
        mov rdx,[rsp+0x48]
        mov r8,[rsp+0x50]
        call write_file
        test rax,rax
        jz .noout
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        call wipe
        mov rcx,[rsp+0x10]
        mov rdx,[rsp+0x18]
        call wipe
        lea rcx,[m_dec1]
        call estr
        xor eax,eax
        jmp .fin
.usage:  lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.nokey:  lea rcx,[m_nokey]
        call estr
        mov eax,2
        jmp .fin
.nocode: lea rcx,[m_nocode]
        call estr
        mov eax,2
        jmp .fin
.noin:   lea rcx,[m_noin]
        call estr
        mov eax,2
        jmp .fin
.badenc: lea rcx,[m_badenc]
        call estr
        mov eax,2
        jmp .fin
.noout:  lea rcx,[m_noout]
        call estr
        mov eax,2
        jmp .fin
.fin:   add rsp,0x60
        pop rbp
        ret

; --- cmd_build
cmd_build:
        push rbp
        sub rsp,0x70
        mov qword [rsp+10h],0
        mov qword [rsp+18h],0
        mov qword [rsp+48h],0
        mov qword [rsp+50h],0
        cmp qword [g_pos],0
        je .u
        mov rcx,[g_pos]
        call read_file
        test rax,rax
        jz .noin
        mov [rsp],rax
        mov [rsp+8],rdx
        cmp rdx,4
        jb .plain
        cmp dword [rax],'AEG0'
        jne .plain
        cmp rdx,44
        jb .badenc
        test r13,r13
        jz .nokey
        cmp r14d,-1
        je .nocode
        mov rcx,r13
        call read_file
        test rax,rax
        jz .nokey
        mov [rsp+10h],rax
        mov [rsp+18h],rdx
        mov rcx,rax
        mov rax,[rsp]
        mov r8d,[rax+8]
        mov r9d,r14d
        lea rax,[rsp+28h]
        push rax
        sub rsp,20h
        call codec_key
        add rsp,20h
        pop rax
        mov rcx,[rsp]
        mov rdx,[rsp+8]
        lea r8,[rsp+28h]
        call codec_decode
        mov [rsp+48h],rax
        mov [rsp+50h],rdx
        lea rcx,[rsp+28h]
        mov edx,32
        call wipe
        cmp qword [rsp+48h],0
        je .badenc
        jmp .compile
.plain:
        mov rax,[rsp]
        mov [rsp+48h],rax
        mov rax,[rsp+8]
        mov [rsp+50h],rax
.compile:
        mov r8,[g_pos+8]
        test r8,r8
        jnz .r8
        lea r8,[d_out]
.r8:    mov rcx,[rsp+48h]
        mov rdx,[rsp+50h]
        call agc_compile
        test eax,eax
        jnz .f
        lea rcx,[m_b1]
        call estr
        xor eax,eax
        jmp .fin
.f:     lea rcx,[m_bfail]
        call estr
        mov eax,2
        jmp .fin
.u:     lea rcx,[c_usage]
        call estr
        mov eax,2
        jmp .fin
.noin:  lea rcx,[m_noin]
        jmp .error
.nokey: lea rcx,[m_nokey]
        jmp .error
.nocode: lea rcx,[m_nocode]
        jmp .error
.badenc: lea rcx,[m_badenc]
.error: call estr
        mov eax,2
.fin:   mov [rsp+60h],rax
        mov rcx,[rsp+10h]
        mov rdx,[rsp+18h]
        call wipe
        mov rcx,[rsp+48h]
        mov rdx,[rsp+50h]
        call wipe
        mov rax,[rsp+60h]
        add rsp,0x70
        pop rbp
        ret

; --- selftest
selftest:
        push rbp
        sub rsp,0xC0
        lea rcx,[t_big]
        mov edx,152
        mov r8,rsp
        call sha256
        mov rsi,rsp
        lea rdi,[h_abc]
        mov ecx,32
        cld
        repe cmpsb
        jne .f1
        lea rcx,[m_t1]
        call estr
        lea rcx,[m_ok]
        call estr
        jmp .t2
.f1:    lea rcx,[m_t1]
        call estr
        lea rcx,[m_fail]
        call estr
        mov eax,1
        jmp .fin
.t2:    mov rcx,rsp
        mov edx,128
        call fill_rand
        mov ecx,144
        call halloc
        mov rbp,rax
        mov rdi,rbp
        mov rsi,rsp
        mov ecx,16
        cld
        rep movsq
        mov rcx,rsp
        mov edx,128
        lea r8,[t_key]
        lea r9,[t_n12]
        call chacha20
        mov rcx,rsp
        mov edx,128
        lea r8,[t_key]
        lea r9,[t_n12]
        call chacha20
        mov rsi,rsp
        mov rdi,rbp
        mov ecx,128
        cld
        repe cmpsb
        jne .f2
        lea rcx,[m_t2]
        call estr
        lea rcx,[m_ok]
        call estr
        xor eax,eax
        jmp .fin
.f2:    lea rcx,[m_t2]
        call estr
        lea rcx,[m_fail]
        call estr
        mov eax,1
        jmp .fin
.fin:   add rsp,0xC0
        pop rbp
        ret

; --- VEH: печать RIP/код/R8 при падении (статический буфер, CONTINUE_SEARCH)
veh:
        push rbx rbp rdi rsi
        mov rbx,rcx
        mov rax,[rbx]
        mov r11,[rax]
        mov rax,[rbx+8]
        mov r10,[rax+0xF8]
        lea rdi,[vehbuf]
        mov dword [rdi],'RIP='
        add rdi,4
        mov rax,r10
        mov esi,16
        call veh_put
        mov byte [rdi],' '
        inc rdi
        mov byte [rdi],'C'
        inc rdi
        mov byte [rdi],'='
        inc rdi
        mov rax,r11
        mov esi,8
        call veh_put
        mov byte [rdi],' '
        inc rdi
        mov byte [rdi],'8'
        inc rdi
        mov byte [rdi],'='
        inc rdi
        mov rax,[rbx+8]
        mov rax,[rax+0xB8]
        mov esi,16
        call veh_put
        mov byte [rdi],' '
        inc rdi
        mov byte [rdi],'9'
        inc rdi
        mov byte [rdi],'='
        inc rdi
        mov rax,[rbx+8]
        mov rax,[rax+0xC0]     ; R9
        mov esi,16
        call veh_put
        mov byte [rdi],' '
        inc rdi
        mov byte [rdi],'1'
        inc rdi
        mov byte [rdi],'='
        inc rdi
        mov rax,[rbx+8]
        mov rax,[rax+0x80]     ; Rcx
        mov esi,16
        call veh_put
        mov byte [rdi],' '
        inc rdi
        mov byte [rdi],'2'
        inc rdi
        mov byte [rdi],'='
        inc rdi
        mov rax,[rbx+8]
        mov rax,[rax+0x88]     ; Rdx
        mov esi,16
        call veh_put
        mov byte [rdi],10
        sub rsp,0x28
        mov ecx,-12
        mov rax,[GetStdHandle]
        call wcall4
        mov rcx,rax
        lea rdx,[vehbuf]
        mov r8d,96
        lea r9,[wr_cnt]
        mov r11d,0
        mov rax,[WriteFile]
        call wcall5
        add rsp,0x28
        pop rsi rdi rbp rbx
        xor eax,eax
        ret

veh_put:
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
        add edx,87
        jmp .st
.dig:   add edx,48
.st:    mov [rdi],dl
        inc rdi
        inc r9d
        cmp r9d,esi
        jb .d
        ret
