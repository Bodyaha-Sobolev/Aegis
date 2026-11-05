; AEGIS — consolidated data of all modules
section '.data' data readable writeable

align 16
gp5     dq 0
gp6     dq 0
gp7     dq 0


align 16
h_base  dq 0
h_ptr   dq 0
h_end   dq 0
ob_buf  dq 0                    ; captured compiler diagnostics
ob_len  dq 0
ob_cap  dq 0
std_in  dq 0
std_out dq 0
std_err dq 0
con_mode dq 0                   ; 1 = консоль подключена


wr_cnt  dq 0

rng_st  dd 0
align 4
sha_k dd 0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5
      dd 0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5
      dd 0xd807aa98,0x12835b01,0x243185be,0x550c7dc3
      dd 0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174
      dd 0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc
      dd 0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da
      dd 0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7
      dd 0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967
      dd 0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13
      dd 0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85
      dd 0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3
      dd 0xd192e819,0xd6990624,0xf40e3585,0x106aa070
      dd 0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5
      dd 0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3
      dd 0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208
      dd 0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2

align 8
codec_alpha rw 1024               ; слот*4+гомофон -> символ (10 бит)
codec_inv   rb 1024               ; символ -> байт
s_alpha  db 'alpha',0
s_rng    db 'rng',0
s_ks     db 'ks',0
s_nonce  db 'nonce',0
s_lzmark db 'lzmark',0




align 16

align 16
g_pos   dq 0,0
g_in    dq 0
g_out   dq 0
g_err   dq 0
c_enc   du 'enc',0
c_dec   du 'dec',0
c_test  du 'test',0
c_build du 'build',0
o_key   db 'key',0
o_code  db 'code',0
d_out   du 'out.exe',0
c_usage db 'AEGIS - usage:',10
        db '  aegis enc <in> <out.aeg> -k <keyfile> [-c 0xABC]',10
        db '  aegis dec <in.aeg> <out> -k <keyfile> -c 0xABC',10
        db '  aegis build <in.ae> [out.exe]',10
        db '  aegis build <in.aeg> [out.exe] -k <keyfile> -c 0xABC',10
        db '  aegis test',10,0
m_enc1  db 'OK: encoded ',0
m_enc2  db ' bytes, dialect 0x',0
m_enc3  db '  -- SAVE THIS CODE!',10,0
m_dec1  db 'OK: decoded',10,0
m_nokey db 'ERROR: key file not found (-k required)',10,0
m_nocode db 'ERROR: dialect code required (-c 0xNNN)',10,0
m_noin  db 'ERROR: input file not found',10,0
m_noout db 'ERROR: cannot write output file',10,0
m_badenc db 'ERROR: wrong key/dialect or corrupted file',10,0
m_b1    db 'OK: built native x64 executable',10,0
m_bfail db 'ERROR: compile failed',10,0
m_ph1 db '[lex ok] ',0
m_ph2 db '[parse ok] ',0
m_ph3 db '[gen ok] ',0
m_ph4 db '[emit ok]',10,0
m_t1    db 'sha256 152B vector      ... ',0
m_t2    db 'chacha20 roundtrip 128B ... ',0
m_t3    db 'codec roundtrip         ... ',0
m_t4    db 'wrong dialect rejected  ... ',0
m_ok    db 'OK',10,0
m_fail  db 'FAIL',10,0
t_big   db 102,110,32,109,97,105,110,40,41,32,123,32,112,114,105,110,116,105,40,52,50,41,59,32,114,101,116,117,114,110,32,48,59,32,125,32,47,47,32,97,101,103,105,115,32,115,97,109,112,108,101,10,0,1,2,3,255,254,128,64,102,110,32,109,97,105,110,40,41,32,123,32,112,114,105,110,116,105,40,52,50,41,59,32,114,101,116,117,114,110,32,48,59,32,125,32,47,47,32,97,101,103,105,115,32,115,97,109,112,108,101,10,0,1,2,3,255,254,128,64,102,110,32,109,97,105,110,40,41,32,123,32,112,114,105,110,116,105,40,52,50,41,59,32,114,101,116,117,114,110,32,48
t_abc   db 'abc'
h_abc   db 0x76,0xc4,0xcc,0x9f,0x06,0x10,0x73,0x48,0x47,0xfd,0x30,0x1c,0x91,0x98,0x9a,0x8b,0xd4,0x7b,0x27,0x2f,0x18,0x36,0xe1,0xa9,0x4d,0x24,0x1c,0x2f,0x65,0x97,0x98,0x50
t_key   db 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31
t_n12   rb 12
t_sample db 'fn main() { printi(42); return 0; } // aegis sample',10
        db 0,1,2,3,255,254,128,64
t_slen = $-t_sample

align 16
g_argc  dq 0
wdump du 'dump.bin',0

qcmp dq 0,0,0,0,0

klen_dbg dq 0,0

ccdbg dq 0,0,0,0

wksenc du 'ksenc.bin',0
wksdec du 'ksdec.bin',0

wkdump du 'kdump.bin',0

veh_r8 dq 0
veh_r9 dq 0
veh_rbp dq 0
veh_rsp dq 0

align 16
stbuf rq 8

t_diffp dq 0,0,0,0,0,0,0,0
wtdbg du 'tdbg.bin',0

align 8
ksbuf1 dq 0,0
ksbuf2 dq 0,0

ubuf1 dq 0,0

w_t3f du 'aegis_t3.tmp',0

vehbuf rb 128

t_lzm db 'lzmark',0
g_tmp8 dq 0
m_rend1 db 'OK: rendered cursed text',10,0
m_dere1 db 'OK: derendered',10,0

astc_val dq 0
agc_src dq 0
agc_len dq 0
agc_out dq 0
agc_tok dq 0
agc_ntok dq 0
agc_ast dq 0
agc_nast dq 0
agc_funcs dq 0
agc_nfunc dq 0
agc_ti dq 0
agc_nvar dq 0
agc_vars dq 0
agc_lastelse dq 0
agc_code dq 0
agc_ccur dq 0
agc_nlbl dq 0
agc_lbls dq 0
agc_patch dq 0
agc_npatch dq 0
agc_cvars dq 0
agc_entry_off dq 0

agc_code_size dd 0
agc_data_size dd 0x300
agc_idata_size dd 0x400
pe_buf dq 0
rt_printi_off dq 0
rt_printc_off dq 0
rt_exit_off dq 0

cli_cmd_test db 'test',0
cli_cmd_build db 'build',0
cli_cmd_enc db 'enc',0
cli_cmd_dec db 'dec',0
cli_cmd_render db 'render',0
cli_cmd_unrender db 'u',0
m_unavailable db 'ERROR: experimental glyph rendering is not available yet',10,0
