; =============================================================================
; AEGIS core — вызовы WinAPI с гарантированным выравниванием стека
; Использование:
;   mov rcx,.. / rdx / r8 / r9            ; 4 аргумента
;   mov r11,p5                             ; 5-й аргумент (wcall5)
;   mov gp5,p5 / gp6,p6 / gp7,p7           ; 5..7-й (wcall7)
;   mov rax,[ИмяAPI]
;   call wcall4 | wcall5 | wcall7
; =============================================================================

wcall4:                         ; rax=fn, rcx rdx r8 r9
        push rbp
        mov rbp,rsp
        and rsp,-16
        sub rsp,0x20
        call rax
        mov rsp,rbp
        pop rbp
        ret

wcall5:                         ; + r11 = 5-й аргумент
        push rbp
        mov rbp,rsp
        and rsp,-16
        sub rsp,0x30
        mov [rsp+0x20],r11
        call rax
        mov rsp,rbp
        pop rbp
        ret

wcall7:                         ; + gp5..gp7 = 5..7-й аргументы
        push rbp
        mov rbp,rsp
        and rsp,-16
        sub rsp,0x40
        mov r10,[gp5]
        mov [rsp+0x20],r10
        mov r10,[gp6]
        mov [rsp+0x28],r10
        mov r10,[gp7]
        mov [rsp+0x30],r10
        call rax
        mov rsp,rbp
        pop rbp
        ret

; Extended Win64 calls used by the native IDE. The complete stack argument
; area is reserved below saved rbp; callbacks can never overwrite the frame.
wcall8:
        push rbp
        mov rbp,rsp
        and rsp,-16
        sub rsp,40h
        mov r10,[gp5]
        mov [rsp+20h],r10
        mov r10,[gp6]
        mov [rsp+28h],r10
        mov r10,[gp7]
        mov [rsp+30h],r10
        mov r10,[gp8]
        mov [rsp+38h],r10
        call rax
        mov rsp,rbp
        pop rbp
        ret

wcall12:
        push rbp
        mov rbp,rsp
        and rsp,-16
        sub rsp,60h
        mov r10,[gp5]
        mov [rsp+20h],r10
        mov r10,[gp6]
        mov [rsp+28h],r10
        mov r10,[gp7]
        mov [rsp+30h],r10
        mov r10,[gp8]
        mov [rsp+38h],r10
        mov r10,[gp9]
        mov [rsp+40h],r10
        mov r10,[gp10]
        mov [rsp+48h],r10
        mov r10,[gp11]
        mov [rsp+50h],r10
        mov r10,[gp12]
        mov [rsp+58h],r10
        call rax
        mov rsp,rbp
        pop rbp
        ret
