; =============================================================================
; AEGIS cursed — проклятая кодировка (v2): байт -> пара «глиф+комбинирующий»
; Пулы без ветвлений: base = 0x2580 + (rng&0x1F)*0x20 -> U+2580..U+2F7F области
; фактически: старшая половина = 0x2500+(rng&0x1F)*0x30 не нужна — просто:
; base = 0x2500 + (rng%0x600); mark = 0x300 + (rng%0x70)
; =============================================================================
section '.text' code readable executable

; --- cursed_tab(r8=key32) -> r12=таблица (256 dword: base<<16|mark)
cursed_tab:
        push rbx rbp r12 r13
        sub rsp,0x20
        mov ecx,0x400
        call halloc
        mov r12,rax
        mov rax,[r8]
        mov [rng_st],eax
        xor rbx,rbx
.gen:   call rng_next
        xor edx,edx
        mov ecx,0x600
        div ecx
        add edx,0x2500
        mov esi,edx             ; base (U+2500..U+2AFF: блоки/геометрия/кань)
        call rng_next
        xor edx,edx
        mov ecx,0x70
        div ecx
        add edx,0x300           ; mark (диакритика U+0300..U+036F)
        shl esi,16
        or esi,edx
        mov [r12+rbx*4],esi
        inc rbx
        cmp rbx,256
        jb .gen
        add rsp,0x20
        pop r13 r12 rbp rbx
        ret

; --- cursed_render(rcx=.aeg, rdx=len, r8=key32) -> rax=utf16, rdx=chars
cursed_render:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x40
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        mov [rbp+0x10],r8
        call cursed_tab
        mov rax,[rbp+8]
        shl rax,2
        add rax,4
        mov rcx,rax
        call halloc
        mov r14,rax
        mov word [r14],0xFEFF
        mov r15,2
        xor r13d,r13d
.enc:   cmp r13,[rbp+8]
        jae .fin
        mov rax,[rbp]
        movzx eax,byte [rax+r13]
        mov edx,[r12+rax*4]
        mov eax,edx
        shr eax,16
        mov [r14+r15*2],ax      ; базовый глиф
        movzx eax,dx
        mov [r14+r15*2+2],ax    ; комбинирующий
        add r15,2
        inc r13
        jmp .enc
.fin:   mov rax,r14
        mov rdx,r15
        add rsp,0x40
        pop r15 r14 r13 r12 rbp rbx
        ret

; --- cursed_derender(rcx=utf16, rdx=chars, r8=key32) -> rax=.aeg, rdx=len | 0,0
cursed_derender:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x40
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        mov [rbp+0x10],r8
        call cursed_tab
        mov rax,[rbp+8]
        sub rax,2
        jbe .bad
        shr rax,1
        mov [rbp+0x20],rax
        mov rcx,rax
        call halloc
        mov r14,rax
        mov [rbp+0x28],rax
        mov r13,2
        xor r15d,r15d
.dec:   cmp r15,[rbp+0x20]
        jae .fin
        mov rcx,[rbp]
        mov eax,r13d
        mov esi,[rcx+rax*2]
        xor rbx,rbx
.srch:  cmp rbx,256
        jae .bad
        cmp [r12+rbx*4],esi
        je .found
        inc rbx
        jmp .srch
.found: mov [r14+r15],bl
        inc r15
        add r13,2
        jmp .dec
.fin:   mov rax,r14
        mov rdx,[rbp+0x20]
        add rsp,0x40
        pop r15 r14 r13 r12 rbp rbx
        ret
.bad:   xor eax,eax
        xor edx,edx
        add rsp,0x40
        pop r15 r14 r13 r12 rbp rbx
        ret
