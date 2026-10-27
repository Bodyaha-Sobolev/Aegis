; AEGIS native compiler. Tokens -> recursive descent -> x64 -> PE32+.
; Internal helpers use a private ABI. agc_compile preserves all Win64 nonvolatiles.
; No bytecode interpreter: the output executes directly on the CPU.
TK_NUM = 256
TK_ID = 257
TK_FN = 258
TK_LET = 259
TK_RETURN = 260
TK_IF = 261
TK_ELSE = 262
TK_WHILE = 263
TK_PRINTI = 264
TK_PRINTC = 265
TK_EXIT = 266
TK_EQ = 267
TK_NE = 268
TK_LE = 269
TK_GE = 270
PE_TEXT_RVA = 1000h
PE_IMPORT_RVA = 11000h
PE_FILE_SIZE = 10800h
MAX_TOKENS = 32768

macro cg bytes& {
        local .payload, .after, .finish
        jmp .after
.payload: db bytes
.finish:
.after: lea rcx,[.payload]
        mov edx,.finish-.payload
        call cg_bytes
}

section '.text' code readable executable

; rcx=source bytes, rdx=length, r8=output UTF-16 path. eax=0 success.
agc_compile:
        push rbx rbp rsi rdi r12 r13 r14 r15
        sub rsp,40h
        mov [agc_recover],rsp
        mov rax,[h_ptr]
        mov [rsp],rax
        mov [agc_src],rcx
        mov [agc_len],rdx
        mov [agc_out],r8
        mov qword [agc_error_pos],0
        mov qword [agc_depth],0
        cmp rdx,100000h
        ja agc_limit
        mov ecx,MAX_TOKENS*32
        call halloc
        mov [agc_tok],rax
        mov ecx,256*32
        call halloc
        mov [agc_funcs],rax
        mov ecx,256*16
        call halloc
        mov [agc_vars],rax
        mov ecx,4096*16
        call halloc
        mov [agc_patch],rax
        mov ecx,10000h
        call halloc
        mov [agc_code],rax
        mov qword [agc_ccur],0
        mov qword [agc_npatch],0
        mov qword [agc_nfunc],0
        mov qword [agc_main],-1
        call agc_lex
        call agc_scan_functions
        lea rcx,[rt_blob]
        mov edx,rt_end-rt_blob
        call cg_bytes
        xor ebx,ebx
.function:
        cmp rbx,[agc_nfunc]
        jae .patches
        mov rax,rbx
        shl rax,5
        add rax,[agc_funcs]
        mov rdx,[agc_ccur]
        mov [rax+24],rdx
        mov rsi,[rax]
        add rsi,64                 ; name -> '(' -> first parameter
        mov qword [agc_nvar],0
        mov qword [agc_slots],0
        mov qword [agc_eval_depth],0
        cg 55h,48h,89h,0E5h,48h,81h,0ECh,00h,08h,00h,00h
        xor r12d,r12d
.params:
        cmp dword [rsi],')'
        je .body
        mov rcx,rsi
        call agc_add_var
        mov eax,r12d
        lea rcx,[cg_param_ops]
        imul eax,3
        add rcx,rax
        mov edx,3
        call cg_bytes
        mov rax,r12
        inc rax
        neg eax
        shl eax,3
        call cg32
        inc r12
        add rsi,32
        cmp dword [rsi],','
        jne .body
        add rsi,32
        jmp .params
.body:
        add rsi,32
        call agc_block
        cg 31h,0C0h,0C9h,0C3h
        inc rbx
        jmp .function
.patches:
        xor ebx,ebx
.patch:
        cmp rbx,[agc_npatch]
        jae .entry
        mov rax,rbx
        shl rax,4
        add rax,[agc_patch]
        mov rcx,[rax+8]
        mov rdx,[rax]
        shl rdx,5
        add rdx,[agc_funcs]
        mov edx,[rdx+24]
        call cg_patch
        inc rbx
        jmp .patch
.entry:
        mov rax,[agc_main]
        shl rax,5
        add rax,[agc_funcs]
        mov edx,[rax+24]
        mov ecx,rt_main_disp-rt_blob
        call cg_patch
        call agc_emit_pe
        test eax,eax
        jz .write_error
        xor eax,eax
        jmp agc_finish
.write_error:
        lea rcx,[m_noout]
        call estr
        mov eax,1
agc_finish:
        mov rsp,[agc_recover]
        mov rdx,[rsp]
        mov [h_ptr],rdx
        add rsp,40h
        pop r15 r14 r13 r12 rdi rsi rbp rbx
        ret

agc_syntax:
        lea rcx,[ce_syntax]
        jmp agc_error
agc_limit:
        lea rcx,[ce_limit]
        jmp agc_fail
agc_error:
        mov rax,[rsi+16]
        mov [agc_error_pos],rax
agc_fail:
        mov [agc_error_message],rcx
        lea rcx,[ce_prefix]
        call estr
        mov rsi,[agc_src]
        xor ebx,ebx
        mov r12d,1
        mov r13d,1
.line:
        cmp rbx,[agc_error_pos]
        jae .report
        cmp rbx,[agc_len]
        jae .report
        cmp byte [rsi+rbx],10
        jne .column
        inc r12
        xor r13d,r13d
.column:
        inc r13
        inc rbx
        jmp .line
.report:
        mov rax,r12
        call agc_print_number
        lea rcx,[ce_colon]
        call estr
        mov rax,r13
        call agc_print_number
        lea rcx,[ce_separator]
        call estr
        mov rcx,[agc_error_message]
        call estr
        lea rcx,[ce_newline]
        call estr
        mov eax,1
        jmp agc_finish

agc_print_number:
        sub rsp,32
        mov rcx,rsp
        call itoa10
        mov rcx,rsp
        call emit
        add rsp,32
        ret

; Token: kind qword, literal qword, source offset qword, length qword.
agc_lex:
        mov rsi,[agc_src]
        xor r12d,r12d
        mov rdi,[agc_tok]
        mov qword [agc_ntok],0
        ; UTF-8 BOM is allowed only at the beginning.
        cmp qword [agc_len],3
        jb .next
        cmp word [rsi],0BBEFh
        jne .next
        cmp byte [rsi+2],0BFh
        jne .next
        mov r12d,3
.next:
        mov [agc_error_pos],r12
        cmp r12,[agc_len]
        jae .eof
        movzx eax,byte [rsi+r12]
        cmp al,32
        je .skip
        cmp al,9
        je .skip
        cmp al,10
        je .skip
        cmp al,13
        je .skip
        mov r13,r12
        cmp al,'/'
        jne .start
        lea rdx,[r12+1]
        cmp rdx,[agc_len]
        jae .start
        cmp byte [rsi+rdx],'/'
        je .comment
        cmp byte [rsi+rdx],'*'
        je .block_comment
.start:
        cmp al,'0'
        jb .identifier
        cmp al,'9'
        ja .identifier
        mov ebx,10
        xor r14d,r14d
        cmp al,'0'
        jne .digit
        lea rdx,[r12+1]
        cmp rdx,[agc_len]
        jae .digit
        mov al,[rsi+rdx]
        or al,20h
        cmp al,'x'
        jne .digit
        add r12,2
        mov ebx,16
        mov r15,r12
.digit:
        cmp r12,[agc_len]
        jae .number_end
        movzx ecx,byte [rsi+r12]
        sub ecx,'0'
        cmp ecx,9
        jbe .digit_value
        cmp ebx,16
        jne .number_end
        movzx ecx,byte [rsi+r12]
        or ecx,20h
        sub ecx,'a'
        cmp ecx,5
        ja .number_end
        add ecx,10
.digit_value:
        mov rax,r14
        mul rbx
        test rdx,rdx
        jnz .bad_number
        add rax,rcx
        jc .bad_number
        mov r14,rax
        inc r12
        jmp .digit
.number_end:
        cmp ebx,16
        jne .emit_number
        cmp r12,r15
        je .bad_number
.emit_number:
        mov eax,TK_NUM
        jmp .store
.identifier:
        movzx ecx,byte [rsi+r12]
        call agc_is_alpha
        test eax,eax
        jz .punct
        inc r12
.id_loop:
        cmp r12,[agc_len]
        jae .id_end
        movzx ecx,byte [rsi+r12]
        call agc_is_alpha
        test eax,eax
        jnz .id_advance
        sub ecx,'0'
        cmp ecx,9
        ja .id_end
.id_advance:
        inc r12
        jmp .id_loop
.id_end:
        mov eax,TK_ID
        xor r14d,r14d
        lea rbx,[agc_keywords]
.keyword:
        cmp dword [rbx],0
        je .store
        mov rdx,r12
        sub rdx,r13
        cmp edx,[rbx+4]
        jne .kw_next
        xor ecx,ecx
.kw_compare:
        mov dl,[rbx+rcx+8]
        lea r8,[rsi+r13]
        cmp dl,[r8+rcx]
        jne .kw_next
        inc ecx
        cmp ecx,[rbx+4]
        jb .kw_compare
        mov eax,[rbx]
        jmp .store
.kw_next:
        add rbx,16
        jmp .keyword
.punct:
        movzx eax,byte [rsi+r12]
        inc r12
        xor r14d,r14d
        cmp al,'='
        je .eq
        cmp al,'!'
        je .ne
        cmp al,'<'
        je .le
        cmp al,'>'
        je .ge
        lea rcx,[agc_punctuation]
.punct_loop:
        cmp byte [rcx],0
        je .bad_char
        cmp al,[rcx]
        je .store
        inc rcx
        jmp .punct_loop
.eq:    mov ebx,TK_EQ
        jmp .double
.ne:    mov ebx,TK_NE
        jmp .double
.le:    mov ebx,TK_LE
        jmp .double
.ge:    mov ebx,TK_GE
.double:
        cmp r12,[agc_len]
        jae .store
        cmp byte [rsi+r12],'='
        jne .store
        inc r12
        mov eax,ebx
.store:
        cmp qword [agc_ntok],MAX_TOKENS-1
        jae agc_limit
        mov [rdi],rax
        mov [rdi+8],r14
        mov [rdi+16],r13
        mov rdx,r12
        sub rdx,r13
        mov [rdi+24],rdx
        add rdi,32
        inc qword [agc_ntok]
        jmp .next
.skip:  inc r12
        jmp .next
.comment:
        inc r12
        cmp r12,[agc_len]
        jae .next
        cmp byte [rsi+r12],10
        jne .comment
        jmp .next
.block_comment:
        add r12,2
.bc_loop:
        lea rdx,[r12+1]
        cmp rdx,[agc_len]
        jae .unterminated
        cmp word [rsi+r12],'*/'
        je .bc_end
        inc r12
        jmp .bc_loop
.bc_end:
        add r12,2
        jmp .next
.eof:
        mov qword [rdi],0
        mov [rdi+16],r12
        ret
.bad_number:
        lea rcx,[ce_number]
        jmp agc_fail
.bad_char:
        lea rcx,[ce_character]
        jmp agc_fail
.unterminated:
        lea rcx,[ce_comment]
        jmp agc_fail

agc_is_alpha:
        mov eax,1
        cmp ecx,'_'
        je .done
        mov edx,ecx
        or edx,20h
        sub edx,'a'
        cmp edx,25
        jbe .done
        xor eax,eax
.done:  ret

; Names are compared by full length and bytes, never by a hash or position.
agc_same_name:                    ; rcx, rdx = token pointers -> eax bool
        mov r8,[rcx+24]
        cmp r8,[rdx+24]
        jne .no
        mov r9,[agc_src]
        mov rcx,[rcx+16]
        mov rdx,[rdx+16]
        add rcx,r9
        add rdx,r9
        xor r9d,r9d
.loop:  cmp r9,r8
        jae .yes
        mov al,[rcx+r9]
        cmp al,[rdx+r9]
        jne .no
        inc r9
        jmp .loop
.yes:   mov eax,1
        ret
.no:    xor eax,eax
        ret

agc_find_fn:                      ; rcx=name token -> rax=index or -1
        push rbx r12
        mov r12,rcx
        xor ebx,ebx
.loop:  cmp rbx,[agc_nfunc]
        jae .no
        mov rax,rbx
        shl rax,5
        add rax,[agc_funcs]
        mov rdx,[rax]
        mov rcx,r12
        call agc_same_name
        test eax,eax
        jnz .yes
        inc rbx
        jmp .loop
.no:    mov rbx,-1
.yes:   mov rax,rbx
        pop r12 rbx
        ret

agc_expect:                       ; eax=expected kind, advances
        cmp [rsi],eax
        jne agc_syntax
        add rsi,32
        ret

agc_scan_functions:
        mov rsi,[agc_tok]
.next:
        cmp dword [rsi],0
        je .done
        mov eax,TK_FN
        call agc_expect
        cmp dword [rsi],TK_ID
        jne agc_syntax
        mov rcx,rsi
        call agc_find_fn
        cmp rax,-1
        jne .duplicate
        cmp qword [agc_nfunc],256
        jae agc_limit
        mov rbx,[agc_nfunc]
        shl rbx,5
        add rbx,[agc_funcs]
        mov [rbx],rsi
        mov rcx,[rsi+16]
        add rcx,[agc_src]
        cmp qword [rsi+24],4
        jne .name_done
        cmp dword [rcx],'main'
        jne .name_done
        mov rax,[agc_nfunc]
        mov [agc_main],rax
.name_done:
        add rsi,32
        mov eax,'('
        call agc_expect
        xor r12d,r12d
        cmp dword [rsi],')'
        je .params_done
.params:
        mov eax,TK_ID
        call agc_expect
        inc r12
        cmp r12,4
        ja agc_limit
        cmp dword [rsi],','
        jne .params_done
        add rsi,32
        jmp .params
.params_done:
        mov [rbx+8],r12
        mov eax,')'
        call agc_expect
        mov [rbx+16],rsi
        mov eax,'{'
        call agc_expect
        mov r12d,1
.skip_body:
        mov eax,[rsi]
        test eax,eax
        jz agc_syntax
        cmp eax,'{'
        jne .close
        inc r12
.close: cmp eax,'}'
        jne .advance
        dec r12
.advance:
        add rsi,32
        test r12,r12
        jnz .skip_body
        inc qword [agc_nfunc]
        jmp .next
.done:
        mov rax,[agc_main]
        cmp rax,-1
        je .no_main
        shl rax,5
        add rax,[agc_funcs]
        cmp qword [rax+8],0
        jne .main_args
        ret
.duplicate:
        lea rcx,[ce_duplicate_fn]
        jmp agc_error
.no_main:
        lea rcx,[ce_main]
        jmp agc_error
.main_args:
        lea rcx,[ce_main_args]
        jmp agc_error

agc_find_var:                     ; rcx=name -> eax slot, -1 if absent
        push rbx r12
        mov r12,rcx
        mov rbx,[agc_nvar]
.loop:  test rbx,rbx
        jz .no
        dec rbx
        mov rax,rbx
        shl rax,4
        add rax,[agc_vars]
        mov rdx,[rax]
        mov rcx,r12
        call agc_same_name
        test eax,eax
        jz .loop
        shl rbx,4
        add rbx,[agc_vars]
        mov eax,[rbx+8]
        jmp .done
.no:    mov eax,-1
.done:  pop r12 rbx
        ret

agc_add_var:
        push rbx
        mov rbx,rcx
        call agc_find_var
        cmp eax,-1
        jne .duplicate
        cmp qword [agc_slots],256
        jae agc_limit
        mov rcx,[agc_nvar]
        shl rcx,4
        add rcx,[agc_vars]
        mov [rcx],rbx
        mov rax,[agc_slots]
        mov [rcx+8],rax
        inc qword [agc_nvar]
        inc qword [agc_slots]
        pop rbx
        ret
.duplicate:
        lea rcx,[ce_duplicate_var]
        jmp agc_error

agc_enter_depth:
        inc qword [agc_depth]
        cmp qword [agc_depth],128
        ja agc_limit
        ret

agc_block:
        push rbx
        call agc_enter_depth
        mov rbx,[agc_nvar]
        mov eax,'{'
        call agc_expect
.loop:
        cmp dword [rsi],'}'
        je .done
        cmp dword [rsi],0
        je agc_syntax
        call agc_statement
        jmp .loop
.done:  add rsi,32
        mov [agc_nvar],rbx
        dec qword [agc_depth]
        pop rbx
        ret

agc_statement:
        push rbx r12 r13
        mov eax,[rsi]
        cmp eax,';'
        je .empty
        cmp eax,'{'
        je .block
        cmp eax,TK_LET
        je .let
        cmp eax,TK_RETURN
        je .return
        cmp eax,TK_IF
        je .if
        cmp eax,TK_WHILE
        je .while
        cmp eax,TK_PRINTI
        je .builtin
        cmp eax,TK_PRINTC
        je .builtin
        cmp eax,TK_EXIT
        je .builtin
        cmp eax,TK_ID
        jne agc_syntax
        cmp dword [rsi+32],'='
        jne .expression
        mov rcx,rsi
        call agc_find_var
        cmp eax,-1
        je agc_undefined_var
        mov ebx,eax
        add rsi,64
        jmp .store
.let:   add rsi,32
        cmp dword [rsi],TK_ID
        jne agc_syntax
        mov r12,rsi
        add rsi,32
        mov eax,'='
        call agc_expect
        mov ecx,1
        call agc_expr
        mov rcx,r12
        call agc_add_var
        mov ebx,eax
        jmp .emit_store
.store:
        mov ecx,1
        call agc_expr
.emit_store:
        cg 48h,89h,85h
        lea eax,[rbx+1]
        neg eax
        shl eax,3
        call cg32
        jmp .semicolon
.return:
        add rsi,32
        mov ecx,1
        call agc_expr
        cg 0C9h,0C3h
        jmp .semicolon
.expression:
        mov ecx,1
        call agc_expr
        jmp .semicolon
.builtin:
        mov ebx,eax
        add rsi,32
        mov eax,'('
        call agc_expect
        mov ecx,1
        call agc_expr
        mov eax,')'
        call agc_expect
        cg 48h,89h,0C1h
        cmp ebx,TK_EXIT
        je .exit
        mov r12d,rt_printi-rt_blob
        cmp ebx,TK_PRINTI
        je .builtin_call
        mov r12d,rt_printc-rt_blob
.builtin_call:
        cg 48h,83h,0ECh,20h,0E8h
        mov eax,r12d
        call cg_relative
        cg 48h,83h,0C4h,20h
        jmp .semicolon
.exit:
        cg 48h,83h,0ECh,20h,41h,0FFh,57h,10h
        jmp .semicolon
.if:
        add rsi,32
        call agc_condition
        call cg_jz
        mov rbx,rax
        call agc_block
        cmp dword [rsi],TK_ELSE
        jne .if_end
        add rsi,32
        cg 0E9h
        call cg_hole
        mov r12,rax
        mov rcx,rbx
        mov rdx,[agc_ccur]
        call cg_patch
        cmp dword [rsi],TK_IF
        jne .else_block
        call agc_statement
        jmp .else_end
.else_block:
        call agc_block
.else_end:
        mov rbx,r12
.if_end:
        mov rcx,rbx
        mov rdx,[agc_ccur]
        call cg_patch
        jmp .done
.while:
        add rsi,32
        mov r12,[agc_ccur]
        call agc_condition
        call cg_jz
        mov rbx,rax
        call agc_block
        cg 0E9h
        mov eax,r12d
        call cg_relative
        mov rcx,rbx
        mov rdx,[agc_ccur]
        call cg_patch
        jmp .done
.block:
        call agc_block
        jmp .done
.semicolon:
        cmp dword [rsi],';'
        jne .done
.empty: add rsi,32
.done:  pop r13 r12 rbx
        ret

agc_condition:
        mov eax,'('
        call agc_expect
        mov ecx,1
        call agc_expr
        mov eax,')'
        jmp agc_expect

; Precedence climbing: comparison=1, add/sub=2, mul/div/rem=3.
agc_expr:
        push rbx r12 r13
        mov r12d,ecx
        call agc_enter_depth
        call agc_unary
.loop:
        mov ebx,[rsi]
        mov eax,ebx
        call agc_precedence
        cmp eax,r12d
        jb .done
        mov r13d,eax
        add rsi,32
        cg 50h
        inc qword [agc_eval_depth]
        lea ecx,[r13+1]
        call agc_expr
        cg 48h,89h,0C1h,58h
        dec qword [agc_eval_depth]
        cmp ebx,'+'
        je .add
        cmp ebx,'-'
        je .sub
        cmp ebx,'*'
        je .mul
        cmp ebx,'/'
        je .div
        cmp ebx,'%'
        je .mod
        cg 48h,39h,0C8h,0Fh
        mov eax,94h
        cmp ebx,TK_EQ
        je .set
        mov eax,95h
        cmp ebx,TK_NE
        je .set
        mov eax,9Ch
        cmp ebx,'<'
        je .set
        mov eax,9Fh
        cmp ebx,'>'
        je .set
        mov eax,9Eh
        cmp ebx,TK_LE
        je .set
        mov eax,9Dh
.set:   call cg8
        cg 0C0h,48h,0Fh,0B6h,0C0h
        jmp .loop
.add:   cg 48h,01h,0C8h
        jmp .loop
.sub:   cg 48h,29h,0C8h
        jmp .loop
.mul:   cg 48h,0Fh,0AFh,0C1h
        jmp .loop
.div:   cg 48h,99h,48h,0F7h,0F9h
        jmp .loop
.mod:   cg 48h,99h,48h,0F7h,0F9h,48h,89h,0D0h
        jmp .loop
.done:  dec qword [agc_depth]
        pop r13 r12 rbx
        ret

agc_precedence:
        cmp eax,'+'
        je .two
        cmp eax,'-'
        je .two
        cmp eax,'*'
        je .three
        cmp eax,'/'
        je .three
        cmp eax,'%'
        je .three
        cmp eax,'<'
        je .one
        cmp eax,'>'
        je .one
        cmp eax,TK_EQ
        jb .zero
        cmp eax,TK_GE
        jbe .one
.zero:  xor eax,eax
        ret
.one:   mov eax,1
        ret
.two:   mov eax,2
        ret
.three: mov eax,3
        ret

agc_unary:
        push rbx
        call agc_enter_depth
        mov ebx,[rsi]
        cmp ebx,'-'
        je .unary
        cmp ebx,'+'
        je .unary
        cmp ebx,'!'
        je .unary
        cmp ebx,TK_NUM
        je .number
        cmp ebx,TK_ID
        je .identifier
        cmp ebx,'('
        jne agc_syntax
        add rsi,32
        mov ecx,1
        call agc_expr
        mov eax,')'
        call agc_expect
        jmp .done
.unary:
        add rsi,32
        call agc_unary
        cmp ebx,'+'
        je .done
        cmp ebx,'-'
        jne .not
        cg 48h,0F7h,0D8h
        jmp .done
.not:   cg 48h,85h,0C0h,0Fh,94h,0C0h,48h,0Fh,0B6h,0C0h
        jmp .done
.number:
        cg 48h,0B8h
        mov rax,[rsi+8]
        call cg64
        add rsi,32
        jmp .done
.identifier:
        cmp dword [rsi+32],'('
        je .call
        mov rcx,rsi
        call agc_find_var
        cmp eax,-1
        je agc_undefined_var
        mov ebx,eax
        cg 48h,8Bh,85h
        lea eax,[rbx+1]
        neg eax
        shl eax,3
        call cg32
        add rsi,32
        jmp .done
.call:  call agc_call
.done:  dec qword [agc_depth]
        pop rbx
        ret

agc_undefined_var:
        lea rcx,[ce_undefined_var]
        jmp agc_error

agc_call:
        push rbx r12 r13
        mov rcx,rsi
        call agc_find_fn
        cmp rax,-1
        je .unknown
        mov rbx,rax
        add rsi,64
        xor r12d,r12d
        cmp dword [rsi],')'
        je .args_done
.args:  cmp r12,4
        jae .arity
        mov ecx,1
        call agc_expr
        cg 50h
        inc qword [agc_eval_depth]
        inc r12
        cmp dword [rsi],','
        jne .args_done
        add rsi,32
        jmp .args
.args_done:
        mov rax,rbx
        shl rax,5
        add rax,[agc_funcs]
        cmp r12,[rax+8]
        jne .arity
        mov eax,')'
        call agc_expect
        cmp r12,4
        jne .pop3
        cg 41h,59h
.pop3:  cmp r12,3
        jb .pop2
        cg 41h,58h
.pop2:  cmp r12,2
        jb .pop1
        cg 5Ah
.pop1:  test r12,r12
        jz .aligned
        cg 59h
.aligned:
        sub [agc_eval_depth],r12
        mov r13d,20h
        test qword [agc_eval_depth],1
        jz .reserve
        mov r13d,28h
.reserve:
        cg 48h,83h,0ECh
        mov eax,r13d
        call cg8
        cg 0E8h
        call cg_hole
        cmp qword [agc_npatch],4096
        jae agc_limit
        mov rcx,[agc_npatch]
        shl rcx,4
        add rcx,[agc_patch]
        mov [rcx],rbx
        mov [rcx+8],rax
        inc qword [agc_npatch]
        cg 48h,83h,0C4h
        mov eax,r13d
        call cg8
        pop r13 r12 rbx
        ret
.unknown:
        lea rcx,[ce_undefined_fn]
        jmp agc_error
.arity: lea rcx,[ce_arity]
        jmp agc_error

; Machine code writers. Multi-byte opcodes are emitted in explicit byte order.
cg_bytes:
        push rsi rdi
        mov rsi,rcx
        mov rcx,rdx
        mov rax,[agc_ccur]
        add rax,rdx
        cmp rax,10000h
        ja agc_limit
        mov rdi,[agc_code]
        add rdi,[agc_ccur]
        mov [agc_ccur],rax
        cld
        rep movsb
        pop rdi rsi
        ret
cg8:    mov edx,1
        jmp cg_value
cg32:   mov edx,4
        jmp cg_value
cg64:   mov edx,8
cg_value:
        push rax
        mov rcx,rsp
        call cg_bytes
        pop rax
        ret
cg_hole:
        mov rax,[agc_ccur]
        push rax
        xor eax,eax
        call cg32
        pop rax
        ret
cg_relative:
        sub eax,dword [agc_ccur]
        sub eax,4
        jmp cg32
cg_patch:
        sub edx,ecx
        sub edx,4
        mov r8,[agc_code]
        mov [r8+rcx],edx
        ret
cg_jz:
        cg 48h,85h,0C0h,0Fh,84h
        jmp cg_hole

; Position independent native support routines, copied into the output .text.
; r15 is the generated executable's IAT base. No pointer into aegis.exe escapes.
rt_blob:
        sub rsp,28h
        db 4Ch,8Dh,3Dh
        dd (PE_IMPORT_RVA+40h)-(PE_TEXT_RVA+($-rt_blob)+4)
        db 0E8h
rt_main_disp:
        dd 0
        mov ecx,eax
        call qword [r15+16]
        int3
rt_printi:
        push rbx
        sub rsp,60h
        mov rbx,rcx
        mov ecx,-11
        call qword [r15]
        mov [rsp+30h],rax
        lea r10,[rsp+60h]
        dec r10
        mov byte [r10],10
        mov rax,rbx
        test rax,rax
        jns .magnitude
        neg rax
.magnitude:
        mov ecx,10
.digit:
        xor edx,edx
        div rcx
        add dl,'0'
        dec r10
        mov [r10],dl
        test rax,rax
        jnz .digit
        test rbx,rbx
        jns .write
        dec r10
        mov byte [r10],'-'
.write:
        mov rcx,[rsp+30h]
        mov rdx,r10
        lea r8,[rsp+60h]
        sub r8,r10
        lea r9,[rsp+38h]
        mov qword [rsp+20h],0
        call qword [r15+8]
        add rsp,60h
        pop rbx
        ret
rt_printc:
        sub rsp,48h
        mov [rsp+30h],cl
        mov ecx,-11
        call qword [r15]
        mov rcx,rax
        lea rdx,[rsp+30h]
        mov r8d,1
        lea r9,[rsp+38h]
        mov qword [rsp+20h],0
        call qword [r15+8]
        add rsp,48h
        ret
rt_end:

; Fixed upper bounds make RVA layout deterministic; the loader sees valid aligned
; sections and a non-overlapping descriptor / IAT / import-name layout.
agc_emit_pe:
        push rbx rsi rdi
        mov ecx,PE_FILE_SIZE
        call halloc
        mov rbx,rax
        mov rdi,rax
        mov ecx,PE_FILE_SIZE/8
        xor eax,eax
        rep stosq
        mov word [rbx],'MZ'
        mov dword [rbx+3Ch],80h
        lea rdi,[rbx+80h]
        mov dword [rdi],'PE'
        mov word [rdi+4],8664h
        mov word [rdi+6],2
        mov word [rdi+14h],0F0h
        mov word [rdi+16h],23h
        add rdi,18h
        mov word [rdi],20Bh
        mov dword [rdi+4],10000h
        mov dword [rdi+8],400h
        mov dword [rdi+10h],PE_TEXT_RVA
        mov dword [rdi+14h],PE_TEXT_RVA
        mov qword [rdi+18h],400000h
        mov dword [rdi+20h],1000h
        mov dword [rdi+24h],200h
        mov word [rdi+28h],6
        mov word [rdi+30h],6
        mov dword [rdi+38h],12000h
        mov dword [rdi+3Ch],400h
        mov word [rdi+44h],3
        mov word [rdi+46h],100h
        mov qword [rdi+48h],100000h
        mov qword [rdi+50h],1000h
        mov qword [rdi+58h],100000h
        mov qword [rdi+60h],1000h
        mov dword [rdi+6Ch],16
        mov dword [rdi+78h],PE_IMPORT_RVA
        mov dword [rdi+7Ch],40
        mov dword [rdi+0D0h],PE_IMPORT_RVA+40h
        mov dword [rdi+0D4h],32
        add rdi,0F0h
        mov dword [rdi],'.tex'
        mov byte [rdi+4],'t'
        mov dword [rdi+8],10000h
        mov dword [rdi+12],PE_TEXT_RVA
        mov dword [rdi+16],10000h
        mov dword [rdi+20],400h
        mov dword [rdi+36],60000020h
        add rdi,40
        mov dword [rdi],'.ida'
        mov word [rdi+4],'ta'
        mov dword [rdi+8],400h
        mov dword [rdi+12],PE_IMPORT_RVA
        mov dword [rdi+16],400h
        mov dword [rdi+20],10400h
        mov dword [rdi+36],0C0000040h
        lea rdi,[rbx+400h]
        mov rsi,[agc_code]
        mov rcx,[agc_ccur]
        rep movsb
        lea rdi,[rbx+10400h]
        lea rsi,[pe_imports]
        mov ecx,pe_imports_end-pe_imports
        rep movsb
        mov rcx,[agc_out]
        mov rdx,rbx
        mov r8d,PE_FILE_SIZE
        call write_file
        pop rdi rsi rbx
        ret

pe_imports:
        dd PE_IMPORT_RVA+40h,0,0,PE_IMPORT_RVA+70h,PE_IMPORT_RVA+40h
        dd 0,0,0,0,0
        db (40h-($-pe_imports)) dup(0)
        dq PE_IMPORT_RVA+80h,PE_IMPORT_RVA+90h,PE_IMPORT_RVA+0A0h,0
        db (70h-($-pe_imports)) dup(0)
        db 'KERNEL32.DLL',0
        db (80h-($-pe_imports)) dup(0)
        dw 0
        db 'GetStdHandle',0
        db (90h-($-pe_imports)) dup(0)
        dw 0
        db 'WriteFile',0
        db (0A0h-($-pe_imports)) dup(0)
        dw 0
        db 'ExitProcess',0
pe_imports_end:

cg_param_ops db 48h,89h,8Dh,48h,89h,95h,4Ch,89h,85h,4Ch,89h,8Dh
agc_punctuation db '(){};,+-*/%',0
agc_keywords:
        dd TK_FN,2
        db 'fn',6 dup(0)
        dd TK_LET,3
        db 'let',5 dup(0)
        dd TK_RETURN,6
        db 'return',2 dup(0)
        dd TK_IF,2
        db 'if',6 dup(0)
        dd TK_ELSE,4
        db 'else',4 dup(0)
        dd TK_WHILE,5
        db 'while',3 dup(0)
        dd TK_PRINTI,6
        db 'printi',2 dup(0)
        dd TK_PRINTC,6
        db 'printc',2 dup(0)
        dd TK_EXIT,4
        db 'exit',4 dup(0)
        dd 0,0,0,0
ce_prefix db 'ERROR ',0
ce_colon db ':',0
ce_separator db ': ',0
ce_newline db 10,0
ce_syntax db 'unexpected token or missing delimiter',0
ce_limit db 'compiler limit exceeded (source/tokens/code/depth/locals/arguments)',0
ce_number db 'integer literal exceeds 64 bits or has no hex digits',0
ce_character db 'invalid character (UTF-8 source expected)',0
ce_comment db 'unterminated block comment',0
ce_main db 'fn main() is required',0
ce_main_args db 'main must have no parameters',0
ce_duplicate_fn db 'function already defined',0
ce_duplicate_var db 'variable already defined in the current scope',0
ce_undefined_var db 'undefined variable',0
ce_undefined_fn db 'undefined function',0
ce_arity db 'wrong number of function arguments (maximum 4)',0

section '.data' data readable writeable
agc_recover dq 0
agc_error_pos dq 0
agc_error_message dq 0
agc_main dq 0
agc_slots dq 0
agc_eval_depth dq 0
agc_depth dq 0
