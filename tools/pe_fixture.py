"""Own synthetic PE export metadata for read-only tests; never loaded/executed."""
import struct


def export_fixture(bits):
    assert bits in (32, 64)
    b = bytearray(0xe00)
    def u16(at, value): struct.pack_into('<H', b, at, value)
    def u32(at, value): struct.pack_into('<I', b, at, value)
    b[:2] = b'MZ'
    u32(0x3c, 0x80)
    b[0x80:0x84] = b'PE\0\0'
    u16(0x84, 0x8664 if bits == 64 else 0x14c)
    u16(0x86, 2)
    u16(0x94, 0xf0 if bits == 64 else 0xe0)
    u16(0x96, 0x2002)
    u16(0x98, 0x20b if bits == 64 else 0x10b)
    if bits == 64: struct.pack_into('<Q', b, 0xb0, 0x140000000)
    else: u32(0xb4, 0x400000)
    u32(0x98+56, 0x3000)
    u32(0x98+60, 0x400)
    u32(0x98+(108 if bits == 64 else 92), 16)
    directory = 0x98+(112 if bits == 64 else 96)
    u32(directory, 0x1000)
    u32(directory+4, 0x300)
    # Also include a normal import table, outside the export directory range.
    # Rust fixtures separately exercise export-only images with no imports.
    u32(directory+8, 0x1300)
    u32(directory+12, 40)
    sections = 0x188 if bits == 64 else 0x178
    for at, name, rva, raw, size, virtual_size, flags in [
        (sections, b'.edata\0\0', 0x1000, 0x400, 0x800, 0x800, 0x40000040),
        (sections+40, b'.text\0\0\0', 0x2000, 0xc00, 0x200, 0x400, 0x60000020),
    ]:
        b[at:at+8] = name
        for offset, value in [(8, virtual_size), (12, rva), (16, size), (20, raw), (36, flags)]: u32(at+offset, value)
    for offset, value in [(12, 0x1160), (16, 7), (20, 5), (24, 3), (28, 0x1100), (32, 0x1120), (36, 0x1140)]: u32(0x400+offset, value)
    for i, rva in enumerate([0x2000, 0, 0x1210, 0x2100, 0x2200]): u32(0x500+4*i, rva)
    for i, rva in enumerate([0x1180, 0x1190, 0x11a0]): u32(0x520+4*i, rva)
    u16(0x544, 2)
    for at, text in [(0x560, b'fixture.dll\0'), (0x580, b'alias\0'), (0x590, b'alpha\0'), (0x5a0, b'forward\0'), (0x610, b'OTHER.#27\0')]: b[at:at+len(text)] = text
    b[0xc00] = 0xc3
    for offset, value in [(0, 0x1340), (12, 0x1380), (16, 0x1340)]: u32(0x700+offset, value)
    if bits == 64: struct.pack_into('<Q', b, 0x740, 0x13a0)
    else: u32(0x740, 0x13a0)
    b[0x780:0x792] = b'FIXTUREIMPORT.dll\0'
    b[0x7a2:0x7ab] = b'external\0'
    return b
