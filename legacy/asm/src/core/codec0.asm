; =============================================================================
; AEGIS слой 0 — кодирование исходников (.aeg)
; Исходник на диске = мусор:
;   - алфавит 1024 знака: 256 байтов x 4 гомофона (Фишер-Йетс от ключа)
;   - юнит u16: 10 бит знак | 4 бита шум | 2 бита шум, XOR keystream ChaCha20
;   - 4096 диалектов (код человека) участвуют в выводе ключа
;   - контекстная зависимость: одинаковые байты -> разные юниты
; Формат: "AEG0" u16 ver=1 u16 flags u32 count u16 units[count] tag[32]
;   tag = SHA256(key32 || units)
; =============================================================================
KDF_ITERS = 1 shl 18             ; ~0.1 c на вывод ключа (медленный KDF)



; --- codec_key(rcx=ключ-файл, rdx=длина, r8=count, r9=диалект, [5]=out32)
; key32 = kdf(ключ-файл || u16le(диалект), salt="AEG0"+u32le(count))
codec_key:
        mov r10,[rsp+0x28]
        push rbx rbp r12 r13
        sub rsp,0x40
        mov rbx,r10             ; out32
        mov r12,rcx             ; ключ-файл
        mov r13,rdx             ; длина
        lea rcx,[rdx+2+16]
        call halloc
        mov [rsp+0x10],rax      ; concat-буфер
        mov rdi,rax
        mov rsi,r12
        mov rcx,r13
        rep movsb
        mov [rdi],r9w           ; u16le(диалект)
        mov dword [rsp+0x18],'AEG0'
        mov [rsp+0x1C],r8d      ; u32le(count)
        mov r12,[rsp+0x10]      ; concat
        sub rsp,0x28
        mov rcx,r12
        lea rdx,[r13+2]
        lea r8,[rsp+0x28+0x18]  ; salt
        mov r9d,8
        mov qword [rsp+0x20],KDF_ITERS
        mov [rsp+0x28],rbx      ; out32
        call kdf
        add rsp,0x28
        mov rcx,[rsp+0x10]
        mov edx,64
        call wipe
        add rsp,0x40
        pop r13 r12 rbp rbx
        ret

; --- codec_tab(rcx=key32): alpha[1024]uw / inv[1024]ub (Фишер-Йетс от ключа)
codec_tab:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x20
        mov rbx,rcx
        lea rcx,[rsp]
        mov rdx,rbx
        lea r8,[s_alpha]
        call derive32
        mov eax,[rsp]
        mov [rng_st],eax
        mov ecx,2048
        call halloc
        mov r12,rax
        xor ecx,ecx
.init:  mov [r12+rcx*2],cx
        inc ecx
        cmp ecx,1024
        jb .init
        mov r13,1023
.fy:    cmp r13,1
        jb .fyd
        call rng_next
        xor edx,edx
        lea ecx,[r13+1]
        div ecx                 ; edx = j
        movzx eax,word [r12+r13*2]
        movzx ecx,word [r12+rdx*2]
        mov [r12+r13*2],cx
        mov [r12+rdx*2],ax
        dec r13
        jmp .fy
.fyd:   xor ecx,ecx
.cp:    movzx eax,word [r12+rcx*2]
        mov [codec_alpha+rcx*2],ax
        mov edx,ecx
        shr edx,2
        mov [codec_inv+rax],dl
        inc ecx
        cmp ecx,1024
        jb .cp
        mov rcx,r12
        mov edx,2048
        call wipe
        add rsp,0x20
        pop r15 r14 r13 r12 rbp rbx
        ret

; --- codec_encode(rcx=src, rdx=len, r8=key32) -> rax=buf, rdx=outlen
; кадр rbp 0x80: [0]=src [8]=len [0x10]=key32 [0x18]=kslen
;   [0x20]=kskey32 [0x40]=nonce12 [0x50]=out; r12=ks r13=i r14=курсор
codec_encode:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x80
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        mov [rbp+0x10],r8
        mov rcx,r8
        call codec_tab
        lea rcx,[rbp+0x20]
        mov rdx,[rbp+0x10]
        lea r8,[s_rng]
        call derive32
        mov eax,[rbp+0x20]
        mov [rng_st],eax
        lea rcx,[rbp+0x20]
        mov rdx,[rbp+0x10]
        lea r8,[s_ks]
        call derive32
        lea rcx,[rbp+0x40]
        mov rdx,[rbp+0x10]
        lea r8,[s_nonce]
        call derive32
        mov rax,[rbp+8]
        shl rax,1
        add rax,8
        mov [rbp+0x18],rax
        mov rcx,rax
        call halloc
        mov r12,rax
        mov qword [r12],0
        mov qword [r12+8],0
        mov rcx,r12
        mov rdx,[rbp+0x18]
        lea r8,[rbp+0x20]
        lea r9,[rbp+0x40]
        call chacha20
        mov rax,[r12]
        mov [ksbuf1],rax
        mov rax,[r12+8]
        mov [ksbuf1+8],rax
        ; LZ-сжатие повторов ссылками (маркер из ключа)
        lea rcx,[rbp+0x60]
        mov rdx,[rbp+0x10]
        lea r8,[s_lzmark]
        call derive32
        movzx eax,byte [rbp+0x60]
        mov [rbp+0x68],rax
        ; LZ отключён до отдельной отладки (AEG_LZ)
        mov qword [rbp+0x68],0
.nolz:  mov rax,[rbp+8]
        shl rax,1
        add rax,64
        mov rcx,rax
        call halloc
        mov [rbp+0x50],rax
        mov r14,rax
        mov dword [r14],'AEG0'
        mov word [r14+4],1
        mov word [r14+6],0
        mov rax,[rbp+8]
        mov [r14+8],eax
        add r14,12
        xor r13d,r13d
.enc:   cmp r13,[rbp+8]
        jae .tags
        mov rax,[rbp]
        movzx ecx,byte [rax+r13]
        shl ecx,2
        call rng_next
        and eax,3
        add ecx,eax
        movzx r15d,word [codec_alpha+rcx*2]
        call rng_next
        and eax,15
        shl eax,10
        or r15d,eax
        call rng_next
        and eax,3
        shl eax,14
        or r15d,eax
        movzx eax,word [r12+r13*2]
        xor r15d,eax
        mov [r14],r15w
        add r14,2
        inc r13
        cmp r13,4
        jne .enc
        mov rax,[rbp+12]
        mov [ubuf1],rax
        mov rax,[rbp+20]
        mov [ubuf1+8],rax
        jmp .enc
.tags:  ; tag = sha256(key32 || units), len = 2*len+32
        mov rax,[rbp+8]
        shl rax,1
        add rax,32
        mov rcx,rax
        call halloc
        mov r15,rax
        mov rdi,r15
        mov rsi,[rbp+0x10]
        mov ecx,4
        rep movsq
        mov rsi,[rbp+0x50]
        add rsi,12
        mov rax,[rbp+8]
        shl rax,1
        mov rcx,rax
        rep movsb
        mov r8,r14
        mov rcx,r15
        lea rdx,[rax+32]
        call sha256
        mov rcx,r12
        mov rdx,[rbp+0x18]
        call wipe
        mov rax,[rbp+8]
        shl rax,1
        add rax,44
        mov rdx,rax
        mov rax,[rbp+0x50]
        add rsp,0x80
        pop r15 r14 r13 r12 rbp rbx
        ret

; --- codec_decode(rcx=enc, rdx=len, r8=key32) -> rax=src, rdx=len | 0,0
codec_decode:
        push rbx rbp r12 r13 r14 r15
        sub rsp,0x80
        mov rbp,rsp
        mov [rbp],rcx
        mov [rbp+8],rdx
        mov [rbp+0x10],r8
        cmp rdx,44
        jb .bad
        cmp dword [rcx],'AEG0'
        jne .bad
        cmp word [rcx+4],1
        jne .bad
        mov eax,[rcx+8]
        mov [rbp+0x18],rax
        lea rcx,[rax*2+44]
        cmp rdx,rcx
        jne .bad
        mov rcx,[rbp+0x10]
        call codec_tab
        lea rcx,[rbp+0x20]
        mov rdx,[rbp+0x10]
        lea r8,[s_ks]
        call derive32
        lea rcx,[rbp+0x40]
        mov rdx,[rbp+0x10]
        lea r8,[s_nonce]
        call derive32
        mov rax,[rbp+0x18]
        shl rax,1
        add rax,8
        mov [rbp+0x58],rax
        mov rcx,rax
        call halloc
        mov r12,rax
        mov qword [r12],0
        mov qword [r12+8],0
        mov rcx,r12
        mov rdx,[rbp+0x58]
        lea r8,[rbp+0x20]
        lea r9,[rbp+0x40]
        call chacha20
        ; тег
        mov rax,[rbp+0x18]
        shl rax,1
        add rax,32
        mov rcx,rax
        call halloc
        mov r15,rax
        mov rdi,r15
        mov rsi,[rbp+0x10]
        mov ecx,4
        rep movsq
        mov rsi,[rbp]
        add rsi,12
        mov rcx,[rbp+0x18]
        shl rcx,1
        rep movsb
        ; hash-буфер из кучи (стек ненадёжен между вызовами)
        mov ecx,32
        call halloc
        mov [rbp+0x60],rax
        mov rcx,r15
        mov rdx,[rbp+0x18]
        shl rdx,1
        add rdx,32
        mov r8,rax
        call sha256
        mov rsi,[rbp+0x60]
        mov rdi,[rbp]
        add rdi,12
        mov rax,[rbp+0x18]
        shl rax,1
        add rdi,rax
        mov ecx,32
        repe cmpsb
        jne .bad
        ; расшифровка
        mov rax,[rbp+0x18]
        add rax,16
        mov rcx,rax
        call halloc
        mov r13,rax
        xor r14d,r14d
.dec:   cmp r14,[rbp+0x18]
        jae .fin
        mov rax,[rbp]
        movzx eax,word [rax+12+r14*2]
        movzx ecx,word [r12+r14*2]
        xor eax,ecx
        and eax,0x3FF
        movzx edx,byte [codec_inv+rax]
        mov [r13+r14],dl
        inc r14
        jmp .dec
.fin:   mov rcx,r12
        mov rdx,[rbp+0x58]

        call wipe
        mov rax,r13
        mov rdx,[rbp+0x18]
        add rsp,0x80
        pop r15 r14 r13 r12 rbp rbx
        ret
.bad:   xor eax,eax
        xor edx,edx
        add rsp,0x80
        pop r15 r14 r13 r12 rbp rbx
        ret
