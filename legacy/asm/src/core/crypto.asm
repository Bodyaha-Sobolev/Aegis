; =============================================================================
; AEGIS crypto — SHA-256, ChaCha20, KDF, RNG, wipe
; Кадры через rbp; все callee-saved сохраняются.
; =============================================================================

; ============ SHA-256 ============
; sha256(rcx=data, rdx=len, r8=out32)
sha256:
        push rbx rbp rdi rsi r12 r13 r14 r15
        sub rsp,0x1C0
        mov rbp,rsp
        mov [rbp+0x1B0],r8
        mov [rbp+0x1A0],rcx
        mov [rbp+0x1A8],rdx
        mov [rbp+0x1B8],rdx
        mov eax,0x6a09e667
        mov [rbp+0x100],eax
        mov eax,0xbb67ae85
        mov [rbp+0x104],eax
        mov eax,0x3c6ef372
        mov [rbp+0x108],eax
        mov eax,0xa54ff53a
        mov [rbp+0x10C],eax
        mov eax,0x510e527f
        mov [rbp+0x110],eax
        mov eax,0x9b05688c
        mov [rbp+0x114],eax
        mov eax,0x1f83d9ab
        mov [rbp+0x118],eax
        mov eax,0x5be0cd19
        mov [rbp+0x11C],eax
.full:
        cmp qword [rbp+0x1A8],64
        jb .tail
        call sha_block
        add qword [rbp+0x1A0],64
        sub qword [rbp+0x1A8],64
        jmp .full
.tail:  mov rdi,rbp
        add rdi,0x120
        mov rsi,[rbp+0x1A0]
        mov rcx,[rbp+0x1A8]
        xor eax,eax
        mov edx,16
.clr:   mov [rdi],rax
        add rdi,8
        dec edx
        jnz .clr
        sub rdi,128
        mov r11,rcx
        rep movsb
        mov byte [rdi],0x80
        mov rax,[rbp+0x1B8]
        shl rax,3
        bswap rax
        cmp r11,56
        jb .short
        lea rdi,[rbp+0x120+120]
        mov [rdi],rax
        jmp .haveblk
.short: lea rdi,[rbp+0x120+56]
        mov [rdi],rax
.haveblk:
        lea rax,[rbp+0x120]
        cmp r11,56
        jb .one
        mov [rbp+0x1A0],rax
        call sha_block
        lea rax,[rbp+0x120+64]
.one:   mov [rbp+0x1A0],rax
        call sha_block
        mov rdi,[rbp+0x1B0]
        xor ecx,ecx
.ser:   mov eax,[rbp+0x100+rcx*4]
        bswap eax
        mov [rdi+rcx*4],eax
        inc ecx
        cmp ecx,8
        jb .ser
        add rsp,0x1C0
        pop r15 r14 r13 r12 rsi rdi rbp rbx
        ret

; внутренний: блок с [rbp+0x1A0]; состояние [rbp+0x100]; w=[rbp]; a..h=r8d..r15d
sha_block:
        mov rsi,[rbp+0x1A0]
        xor ecx,ecx
.ld:    mov eax,[rsi+rcx*4]
        bswap eax
        mov [rbp+rcx*4],eax
        inc ecx
        cmp ecx,16
        jb .ld
.ext:           cmp ecx,64
        jae .go
        mov r8d,[rbp+rcx*4-60]
        mov r9d,r8d
        ror r9d,7
        mov r10d,r8d
        ror r10d,18
        xor r9d,r10d
        shr r8d,3
        xor r9d,r8d
        mov r11d,[rbp+rcx*4-8]
        mov r8d,r11d
        ror r8d,17
        mov r10d,r11d
        ror r10d,19
        xor r8d,r10d
        shr r11d,10
        xor r8d,r11d
        mov eax,[rbp+rcx*4-64]
        add eax,r9d
        add eax,[rbp+rcx*4-28]
        add eax,r8d
        mov [rbp+rcx*4],eax
        inc ecx
        jmp .ext
.go:    mov r8d,[rbp+0x100]
        mov r9d,[rbp+0x104]
        mov r10d,[rbp+0x108]
        mov r11d,[rbp+0x10C]
        mov r12d,[rbp+0x110]
        mov r13d,[rbp+0x114]
        mov r14d,[rbp+0x118]
        mov r15d,[rbp+0x11C]
        xor ebx,ebx
.rnd:   mov eax,r12d
        mov edx,r12d
        ror eax,6
        ror edx,11
        xor eax,edx
        mov edx,r12d
        ror edx,25
        xor eax,edx
        mov edx,r12d
        and edx,r13d
        mov esi,r12d
        not esi
        and esi,r14d
        xor edx,esi
        add eax,edx
        add eax,r15d
        mov edx,[sha_k+rbx*4]
        add eax,edx
        mov edx,[rbp+rbx*4]
        add eax,edx
        mov edx,r8d
        mov esi,r8d
        ror edx,2
        ror esi,13
        xor edx,esi
        mov esi,r8d
        ror esi,22
        xor edx,esi
        mov esi,r8d
        and esi,r9d
        mov edi,r8d
        and edi,r10d
        or esi,edi
        mov edi,r9d
        and edi,r10d
        or esi,edi
        add edx,esi
        mov r15d,r14d
        mov r14d,r13d
        mov r13d,r12d
        mov r12d,r11d
        add r12d,eax          ; e = d + T1
        mov r11d,r10d
        mov r10d,r9d
        mov r9d,r8d
        add eax,edx
        mov r8d,eax
        inc ebx
        cmp ebx,64
        jb .rnd
        add [rbp+0x100],r8d
        add [rbp+0x104],r9d
        add [rbp+0x108],r10d
        add [rbp+0x10C],r11d
        add [rbp+0x110],r12d
        add [rbp+0x114],r13d
        add [rbp+0x118],r14d
        add [rbp+0x11C],r15d
        ret

; ============ ChaCha20 ============
; chacha20(rcx=данные(на месте), rdx=len, r8=key32, r9=nonce12)
; кадр rbp: [0x00]=state[16] [0x40]=block[16] [0x80]=ptr [0x88]=len
;           [0xC0]=x14 [0xC4]=x15
macro QR a,b,c,d {
        add a,b
        xor d,a
        rol d,16
        add c,d
        xor b,c
        rol b,12
        add a,b
        xor d,a
        rol d,8
        add c,d
        xor b,c
        rol b,7
}
macro CCDR {
        QR eax,esi,r10d,r14d
        QR ebx,edi,r11d,r15d
        QR ecx,r8d,r12d,dword [rbp+0xC0]
        QR edx,r9d,r13d,dword [rbp+0xC4]
        QR eax,edi,r12d,dword [rbp+0xC4]
        QR ebx,r8d,r13d,r14d
        QR ecx,r9d,r10d,r15d
        QR edx,esi,r11d,dword [rbp+0xC0]
}
chacha20:
        push rbx rbp rdi rsi r12 r13 r14 r15
        sub rsp,0x100
        mov rbp,rsp
        mov [rbp+0x80],rcx
        mov [rbp+0x88],rdx

        mov dword [rbp],0x61707865
        mov dword [rbp+4],0x3320646e
        mov dword [rbp+8],0x79622d32
        mov dword [rbp+12],0x6b206574
        xor ecx,ecx
.mk:    mov eax,[r8+rcx*4]
        mov [rbp+16+rcx*4],eax
        inc ecx
        cmp ecx,8
        jb .mk
        mov dword [rbp+48],1
        mov eax,[r9]
        mov [rbp+52],eax
        mov eax,[r9+4]
        mov [rbp+56],eax
        mov eax,[r9+8]
        mov [rbp+60],eax
.blk:   cmp qword [rbp+0x88],0
        je .done
        mov edx,[rbp+56]
        mov [rbp+0xC0],edx      ; x14
        mov edx,[rbp+60]
        mov [rbp+0xC4],edx      ; x15
        mov eax,[rbp]
        mov ebx,[rbp+4]
        mov ecx,[rbp+8]
        mov edx,[rbp+12]
        mov esi,[rbp+16]
        mov edi,[rbp+20]
        mov r8d,[rbp+24]
        mov r9d,[rbp+28]
        mov r10d,[rbp+32]
        mov r11d,[rbp+36]
        mov r12d,[rbp+40]
        mov r13d,[rbp+44]
repeat 10
        CCDR
end repeat
        mov [rbp+0x40],eax
        mov [rbp+0x44],ebx
        mov [rbp+0x48],ecx
        mov [rbp+0x4C],edx
        mov [rbp+0x50],esi
        mov [rbp+0x54],edi
        mov [rbp+0x58],r8d
        mov [rbp+0x5C],r9d
        mov [rbp+0x60],r10d
        mov [rbp+0x64],r11d
        mov [rbp+0x68],r12d
        mov [rbp+0x6C],r13d
        mov [rbp+0x70],r14d
        mov [rbp+0x74],r15d
        mov eax,[rbp+0xC0]
        mov [rbp+0x78],eax
        mov eax,[rbp+0xC4]
        mov [rbp+0x7C],eax
        xor ecx,ecx
.ad:    mov edx,[rbp+rcx*4]
        add [rbp+0x40+rcx*4],edx
        inc ecx
        cmp ecx,16
        jb .ad
        mov rcx,[rbp+0x88]
        cmp rcx,64
        jbe .k
        mov ecx,64
.k:     mov rax,rcx
        mov rdi,[rbp+0x80]
        lea rsi,[rbp+0x40]
.xr:    mov dl,[rsi]
        xor [rdi],dl
        inc rdi
        inc rsi
        dec rcx
        jnz .xr
        add [rbp+0x80],rax
        sub [rbp+0x88],rax
        inc dword [rbp+48]
        jmp .blk
.done:  xor eax,eax
        mov [rbp],rax
        mov [rbp+8],rax
        mov [rbp+16],rax
        mov [rbp+24],rax
        mov [rbp+32],rax
        mov [rbp+40],rax
        mov [rbp+48],rax
        mov [rbp+56],rax
        add rsp,0x100
        pop r15 r14 r13 r12 rsi rdi rbp rbx
        ret

; ============ KDF (медленный) ============
; kdf(rcx=key, rdx=keylen, r8=salt, r9=saltlen, [5]=итераций, [6]=out32)
kdf:
        mov r10,[rsp+0x28]
        mov r11,[rsp+0x30]
        push rbx rbp r12 r13
        sub rsp,0x48
        mov rbx,r11             ; out
        mov r12,r10             ; iters
        mov rax,rdx
        add rax,r9
        add rax,64
        mov [rsp+8],rax         ; размер конката
        push rcx rdx r8 r9
        mov rcx,rax
        call halloc
        mov r13,rax
        pop r9 r8 rdx rcx
        mov rdi,r13
        mov rsi,rcx
        mov rcx,rdx
        rep movsb
        mov rsi,r8
        mov rcx,r9
        rep movsb
        mov rcx,r13
        mov rdx,[rsp+8]
        sub rdx,64
        lea r8,[rsp+0x10]
        call sha256
.it:    test r12,r12
        jz .fin
        lea rcx,[rsp+0x10]
        mov edx,32
        lea r8,[rsp+0x10]
        call sha256
        dec r12
        jmp .it
.fin:   mov rdi,rbx
        lea rsi,[rsp+0x10]
        mov ecx,4
        rep movsq
        xor eax,eax
        mov [rsp+0x10],rax
        mov [rsp+0x18],rax
        mov [rsp+0x20],rax
        mov [rsp+0x28],rax
        add rsp,0x48
        pop r13 r12 rbp rbx
        ret

; ============ derive32(rcx=out32, rdx=master32, r8=назначение z) ============
cld
derive32:
        push rbx rbp r12 r13
        sub rsp,0x40
        mov rbx,rcx
        xor eax,eax
.pl:    cmp byte [r8+rax],0
        je .pe
        inc rax
        jmp .pl
.pe:    mov r12,rax             ; plen
        lea r13,[rax+32+1]
        mov rcx,r13
        push rdx r8
        call halloc
        pop r8 rdx
        mov r13,rax             ; buf
        mov rdi,r13
        mov rsi,rdx
        mov ecx,4
        rep movsq
        mov rsi,r8
        lea rcx,[r12+1]
        rep movsb
        mov rcx,r13
        lea rdx,[r12+33]
        lea r8,[rsp]
        call sha256
        mov rdi,rbx
        lea rsi,[rsp]
        mov ecx,4
        rep movsq
        xor eax,eax
        mov [rsp],rax
        mov [rsp+8],rax
        mov [rsp+0x10],rax
        mov [rsp+0x18],rax
        add rsp,0x40
        pop r13 r12 rbp rbx
        ret

; ============ fill_rand(rcx=buf, rdx=len) — системный RNG ============
fill_rand:
        mov rax,[SystemFunction036]
        call wcall4
        ret

; ============ wipe(rcx=ptr, rdx=len) ============
cld
wipe:   push rdi
        mov rdi,rcx
        mov rcx,rdx
        xor eax,eax
        cld
        rep stosb
        pop rdi
        ret

; ============ xorshift32 (шум кодека) ============
; rng_next() -> eax
rng_next:
        mov eax,[rng_st]
        mov edx,eax
        shl edx,13
        xor eax,edx
        mov edx,eax
        shr edx,17
        xor eax,edx
        mov edx,eax
        shl edx,5
        xor eax,edx
        mov [rng_st],eax
        ret
