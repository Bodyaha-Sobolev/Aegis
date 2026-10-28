"""Verify the load address of a newly created test child under mandatory ASLR.

Uses documented Windows debug events, without reading or modifying process memory.
Only the child created here is debugged; no system mitigation settings are changed.
Requires 64-bit Python on Windows 10/11, like the native compiler tests.
"""
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
import time


class StartupInfo(c.Structure):
    _fields_ = [
        ('cb', w.DWORD), ('reserved', w.LPWSTR), ('desktop', w.LPWSTR),
        ('title', w.LPWSTR), ('x', w.DWORD), ('y', w.DWORD),
        ('width', w.DWORD), ('height', w.DWORD), ('columns', w.DWORD),
        ('rows', w.DWORD), ('fill', w.DWORD), ('flags', w.DWORD),
        ('show', w.WORD), ('reserved_size', w.WORD), ('reserved_ptr', c.c_void_p),
        ('stdin', w.HANDLE), ('stdout', w.HANDLE), ('stderr', w.HANDLE),
    ]


class StartupInfoEx(c.Structure):
    _fields_ = [('startup', StartupInfo), ('attributes', c.c_void_p)]


class ProcessInfo(c.Structure):
    _fields_ = [('process', w.HANDLE), ('thread', w.HANDLE),
                ('pid', w.DWORD), ('tid', w.DWORD)]


class CreateProcessInfo(c.Structure):
    _fields_ = [('file', w.HANDLE), ('process', w.HANDLE), ('thread', w.HANDLE),
                ('base', c.c_void_p), ('debug_offset', w.DWORD),
                ('debug_size', w.DWORD), ('tls', c.c_void_p),
                ('start', c.c_void_p), ('name', c.c_void_p), ('unicode', w.WORD)]


class EventData(c.Union):
    # EXCEPTION_DEBUG_INFO is the largest union member: 160 bytes on x64.
    _fields_ = [('create', CreateProcessInfo), ('words', c.c_uint64 * 20),
                ('first_dword', w.DWORD), ('file', w.HANDLE)]


class DebugEvent(c.Structure):
    _fields_ = [('code', w.DWORD), ('pid', w.DWORD), ('tid', w.DWORD),
                ('data', EventData)]


class JobBasicLimits(c.Structure):
    _fields_ = [('process_time', c.c_int64), ('job_time', c.c_int64),
                ('flags', w.DWORD), ('minimum_working_set', c.c_size_t),
                ('maximum_working_set', c.c_size_t), ('active_processes', w.DWORD),
                ('affinity', c.c_size_t), ('priority', w.DWORD), ('scheduling', w.DWORD)]


class JobExtendedLimits(c.Structure):
    _fields_ = [('basic', JobBasicLimits), ('io_counters', c.c_uint64 * 6),
                ('process_memory', c.c_size_t), ('job_memory', c.c_size_t),
                ('peak_process_memory', c.c_size_t), ('peak_job_memory', c.c_size_t)]


def run_relocated(exe, timeout=15, *, memory_limit=None):
    """Return (observed image base, exit code) for this newly created child."""
    assert c.sizeof(c.c_void_p) == 8, 'ASLR verification requires 64-bit Python'
    assert c.sizeof(StartupInfo) == 104 and c.sizeof(DebugEvent) == 176
    assert c.sizeof(JobBasicLimits) == 64 and c.sizeof(JobExtendedLimits) == 144
    if memory_limit is not None and not 16*1024*1024 <= memory_limit <= 1024*1024*1024:
        raise ValueError('test child commit limit must be between 16 MiB and 1 GiB')
    k = c.WinDLL('kernel32', use_last_error=True)
    signatures = {
        'InitializeProcThreadAttributeList': ([c.c_void_p, w.DWORD, w.DWORD, c.POINTER(c.c_size_t)], w.BOOL),
        'UpdateProcThreadAttribute': ([c.c_void_p, w.DWORD, c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p, c.c_void_p], w.BOOL),
        'DeleteProcThreadAttributeList': ([c.c_void_p], None),
        'CreateProcessW': ([w.LPCWSTR, w.LPWSTR, c.c_void_p, c.c_void_p, w.BOOL, w.DWORD, c.c_void_p, w.LPCWSTR, c.POINTER(StartupInfoEx), c.POINTER(ProcessInfo)], w.BOOL),
        'WaitForDebugEvent': ([c.POINTER(DebugEvent), w.DWORD], w.BOOL),
        'ContinueDebugEvent': ([w.DWORD, w.DWORD, w.DWORD], w.BOOL),
        'TerminateProcess': ([w.HANDLE, w.UINT], w.BOOL),
        'DebugActiveProcessStop': ([w.DWORD], w.BOOL),
        'WaitForSingleObject': ([w.HANDLE, w.DWORD], w.DWORD),
        'CloseHandle': ([w.HANDLE], w.BOOL),
        'CreateJobObjectW': ([c.c_void_p, w.LPCWSTR], w.HANDLE),
        'SetInformationJobObject': ([w.HANDLE, c.c_int, c.c_void_p, w.DWORD], w.BOOL),
        'AssignProcessToJobObject': ([w.HANDLE, w.HANDLE], w.BOOL),
    }
    for name, (args, result) in signatures.items():
        fn = getattr(k, name)
        fn.argtypes, fn.restype = args, result

    def checked(result):
        if not result:
            raise c.WinError(c.get_last_error())

    size = c.c_size_t()
    # The sizing call is documented to fail with ERROR_INSUFFICIENT_BUFFER.
    k.InitializeProcThreadAttributeList(None, 1, 0, c.byref(size))
    assert c.get_last_error() == 122 and size.value > 0
    attributes = c.create_string_buffer(size.value)
    checked(k.InitializeProcThreadAttributeList(attributes, 1, 0, c.byref(size)))
    info = ProcessInfo()
    finished = False
    job = None
    try:
        if memory_limit is not None:
            job = k.CreateJobObjectW(None, None)
            checked(job)
            limits = JobExtendedLimits()
            # PROCESS_MEMORY | KILL_ON_JOB_CLOSE. A fresh unnamed job owns only
            # the fixture child created below; no PID lookup or global settings.
            # https://learn.microsoft.com/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information
            limits.basic.flags = 0x100 | 0x2000
            limits.process_memory = memory_limit
            checked(k.SetInformationJobObject(job, 9, c.byref(limits), c.sizeof(limits)))
        # ForceRelocateImages + RequireRelocations, BottomUpASLR, HighEntropyASLR.
        # UpdateProcThreadAttribute / PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY.
        policy = c.c_uint64(0x300 | 0x10000 | 0x100000)
        checked(k.UpdateProcThreadAttribute(attributes, 0, 0x20007,
                c.byref(policy), c.sizeof(policy), None, None))
        startup = StartupInfoEx()
        startup.startup.cb = c.sizeof(startup)
        startup.attributes = c.cast(attributes, c.c_void_p)
        path = str(Path(exe).resolve(strict=True))
        command = c.create_unicode_buffer('"' + path + '"')
        # DEBUG_ONLY_THIS_PROCESS | EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW.
        flags = 0x2 | 0x80000 | 0x8000000
        checked(k.CreateProcessW(path, command, None, None, False, flags,
                                None, None, c.byref(startup), c.byref(info)))
        if job:
            # The debug-created child cannot run past its initial debug event
            # before ContinueDebugEvent below, so application allocations are capped.
            checked(k.AssignProcessToJobObject(job, info.process))
        deadline = time.monotonic() + timeout
        base = None
        initial_breakpoint = False
        while time.monotonic() < deadline:
            event = DebugEvent()
            if not k.WaitForDebugEvent(c.byref(event), 250):
                error = c.get_last_error()
                if error == 121:  # ERROR_SEM_TIMEOUT
                    continue
                raise c.WinError(error)
            assert event.pid == info.pid, 'unexpected debug target'
            status = 0x10002  # DBG_CONTINUE
            exit_code = None
            if event.code == 3:  # CREATE_PROCESS_DEBUG_EVENT
                base = event.data.create.base
                if event.data.create.file:
                    checked(k.CloseHandle(event.data.create.file))
            elif event.code == 6:  # LOAD_DLL_DEBUG_EVENT
                if event.data.file:
                    checked(k.CloseHandle(event.data.file))
            elif event.code == 1:  # EXCEPTION_DEBUG_EVENT
                if event.data.first_dword == 0x80000003 and not initial_breakpoint:
                    initial_breakpoint = True
                else:
                    status = 0x80010001  # DBG_EXCEPTION_NOT_HANDLED
            elif event.code == 5:  # EXIT_PROCESS_DEBUG_EVENT
                exit_code = event.data.first_dword
            checked(k.ContinueDebugEvent(event.pid, event.tid, status))
            if exit_code is not None:
                if k.WaitForSingleObject(info.process, 5000) != 0:
                    raise TimeoutError('ASLR child did not finish after exit event')
                finished = True
                assert base is not None, 'no image-load event observed'
                return base, exit_code
        raise TimeoutError('ASLR test child exceeded time limit')
    finally:
        if info.process:
            if not finished:
                k.TerminateProcess(info.process, 125)
                k.DebugActiveProcessStop(info.pid)
                k.WaitForSingleObject(info.process, 5000)
            k.CloseHandle(info.thread)
            k.CloseHandle(info.process)
        if job:
            k.CloseHandle(job)
        k.DeleteProcThreadAttributeList(attributes)
