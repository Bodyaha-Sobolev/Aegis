; =============================================================================
; AEGIS LZ — сжатие повторов ссылками (слой 0, перед кодированием)
; Токен: [marker][b1][b2]: b1=((len-4)<<4)|((dist-1)>>8), b2=(dist-1)&0xFF
;   len 4..19, dist 1..4096. Escape: [marker][0x00] = литерал marker.
; marker = derive32(key32,"lzmark")[0] — рандомизированный ID на файл/ключ.
; Флаг в заголовке .aeg: flags bit0=1 (LZ), bits8-15 = marker.
; =============================================================================


; --- lz_pack(rcx=src, rdx=len, r8=key32) -> rax=out, rdx=outlen | 0,0
; кадр rbp 0x60: [0]=src [8]=len [0x10]=marker [0x18]=out [0x20]=outlen
;   [0x28]=srcpos [0x30]=tab(65536*4) r12=tab r13=i r14=oc (out cursor)
cld
lz_pack:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x60
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        lea rcx,[rbp+0x30]
        mov rdx,r8
        lea r8,[s_lzmark]
        call derive32
        movzx eax,byte [rbp+0x30]
        mov [rbp+0x10],rax
        mov rax,[rbp+8]
        add rax,64
        mov rcx,rax
        call halloc
        mov [rbp+0x18],rax
        mov r14,rax
        mov qword [rbp+0x20],0
        xor r13d,r13d
.lp:    cmp r13,[rbp+8]
        jae .fin
        ; поиск лучшего матча назад: dist 1..4096, len 4..19
        mov r15d,0              ; best len
        mov r12d,0              ; best dist
        mov r8d,1               ; dist
.srch:  cmp r8d,4096
        ja .srdone
        mov eax,r13d
        sub eax,r8d
        jc .srdone
        mov r9d,0               ; тек. длина совпадения
.mlen:  mov eax,r13d
        add eax,r9d
        cmp eax,[rbp+8]
        jae .mdone
        cmp r9d,19
        jae .mdone
        mov rcx,[rbp]
        mov eax,r13d
        add eax,r9d
        movzx eax,byte [rcx+rax]
        mov edx,r13d
        sub edx,r8d
        add edx,r9d
        movzx edx,byte [rcx+rdx]
        cmp eax,edx
        jne .mdone
        inc r9d
        jmp .mlen
.mdone: cmp r9d,r15d
        jbe .srnext
        cmp r9d,4
        jb .srnext
        mov r15d,r9d
        mov r12d,r8d
.srnext:inc r8d
        jmp .srch
.srdone:
        cmp r15d,4
        jb .lit
        ; токен
        mov rax,[rbp+0x10]
        mov [r14],al
        mov eax,r15d
        sub eax,4
        shl eax,4
        mov ecx,r12d
        dec ecx
        mov edx,ecx
        shr edx,8
        or eax,edx
        mov [r14+1],al
        mov [r14+2],cl
        add r14,3
        add qword [rbp+0x20],3
        add r13,r15
        jmp .lp
.lit:   mov rcx,[rbp]
        movzx eax,byte [rcx+r13]
        cmp eax,[rbp+0x10]
        je .esc
        mov [r14],al
        inc r14
        inc qword [rbp+0x20]
        inc r13
        jmp .lp
.esc:   mov rax,[rbp+0x10]
        mov [r14],al
        mov byte [r14+1],0
        add r14,2
        add qword [rbp+0x20],2
        inc r13
        jmp .lp
.fin:   mov rax,[rbp+0x18]
        mov rdx,[rbp+0x20]
        add rsp,0x40
        pop r15 r14 r13 r12 rbp rbx
        ret

; --- lz_unpack(rcx=in, rdx=len, r8=marker) -> rax=out, rdx=outlen
; out максимум 2*len+64
cld
lz_unpack:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x50
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        lea rcx,[rbp+0x20]
        mov rdx,r8
        lea r8,[s_lzmark]
        call derive32
        movzx eax,byte [rbp+0x20]
        mov [rbp+0x10],rax
        mov rdx,[rbp+8]
        mov rax,rdx
        shl rax,1
        add rax,64
        mov rcx,rax
        call halloc
        mov [rbp+0x18],rax
        mov r14,rax
        mov qword [rbp+0x20],0
        xor r13d,r13d
.lp:    cmp r13,[rbp+8]
        jae .fin
        mov rcx,[rbp]
        movzx eax,byte [rcx+r13]
        cmp rax,[rbp+0x10]
        je .tok
        mov [r14],al
        inc r14
        inc qword [rbp+0x20]
        inc r13
        jmp .lp
.tok:   inc r13
        cmp r13,[rbp+8]
        jae .fin
        mov rcx,[rbp]
        movzx eax,byte [rcx+r13]
        test eax,eax
        jnz .match
        ; escape: литерал marker
        mov rax,[rbp+0x10]
        mov [r14],al
        inc r14
        inc qword [rbp+0x20]
        inc r13
        jmp .lp
.match: movzx eax,byte [rcx+r13]        ; b1
        movzx r9d,byte [rcx+r13+1]      ; b2
        inc r13
        inc r13
        mov edx,eax
        shr edx,4
        add edx,4                       ; len
        mov ecx,eax
        and ecx,15
        shl ecx,8
        or ecx,r9d
        inc ecx                         ; dist
        ; копировать dist-назад len байт
        mov rax,[rbp+0x20]
        sub rax,rcx
        add rax,[rbp+0x18]              ; источник в out
        xor r8d,r8d
.cp:    cmp r8d,edx
        jae .cpd
        mov r10b,[rax+r8]
        mov [r14],r10b
        inc r14
        inc qword [rbp+0x20]
        inc r8d
        jmp .cp
.cpd:   jmp .lp
.fin:   mov rax,[rbp+0x18]
        mov rdx,[rbp+0x20]
        add rsp,0x30
        pop r15 r14 r13 r12 rbp rbx
        ret

; dval — печать 16 hex из [ccdbg], все регистры сохраняет
dval:
        push rax rcx rdx r8 r9 r10 r11 rdi rsi
        sub rsp,0x48
        lea rdi,[rsp]
        mov rax,[ccdbg]
        mov esi,16
        call veh_put2
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

veh_put2:
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
