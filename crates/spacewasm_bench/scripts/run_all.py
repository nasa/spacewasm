#!/usr/bin/env python3

# A script that reads ELF section sizes and runs the coremark benchmark and outputs JSON.
# This script should be run from the workspace root.
#
# Copyright 2026 California Institute of Technology
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# <http://www.apache.org/licenses/LICENSE-2.0>

import queue
import json
import subprocess
import sys
import os
import time
from threading import Thread

qemu_flags = "-nographic -semihosting-config enable=on,target=native -kernel"

qemu_commands = {
    "riscv32imc-unknown-none-elf": "qemu-system-riscv32 -icount shift={shift} -bios none -cpu rv32 -machine virt -m 256M {flags}",
    "riscv64gc-unknown-none-elf": "qemu-system-riscv64 -icount shift={shift} -bios none -cpu rv64 -machine virt -m 256M {flags}",
    "thumbv7m-none-eabi": "qemu-system-arm -icount shift={shift} -cpu cortex-m4 -machine netduinoplus2 {flags}",
    "thumbv7em-none-eabihf": "qemu-system-arm -icount shift={shift} -cpu cortex-m4 -machine netduinoplus2 {flags}",
}
qemu_info = {
    "riscv32imc-unknown-none-elf": "rv32 / virt",
    "riscv64gc-unknown-none-elf": "rv64 / virt",
    "thumbv7m-none-eabi": "cortex-m4 / netduinoplus2 @ 1GHz",
    "thumbv7em-none-eabihf": "cortex-m4 / netduinoplus2 @ 1GHz",
}
triples = list(qemu_commands.keys())
elf_sections = [".text", ".rodata", ".data", ".bss"]

def get_coremark(triple, q, shift):
    """runs QEMU over target triple"""

    command = [
        *qemu_commands[triple].format(shift=shift, flags=qemu_flags).split(" "),
        f"target/{triple}/release/spacewasm_bench"
    ]

    sys.stderr.write(" ".join(command) + "\n")
    sys.stderr.flush()

    start_time = time.time()
    proc = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    full_time = time.time() - start_time


    sys.stderr.write(proc.stderr.decode())
    sys.stderr.flush()

    output = float(proc.stdout.decode()) * (2 ** shift)

    q.put((triple, output, full_time))

def get_sizes(triple):
    """build target triple and runs cargo readobj over it"""

    out = {}

    command = ["cargo", "readobj", "-q", "--release", "-p", "spacewasm_bench", "--target", triple, "--", "--sections"]
    proc = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    sys.stderr.write(" ".join(command) + "\n")
    sys.stderr.flush()

    out["details"] = proc.stdout.decode()

    proc = subprocess.run(
        [*command, "--elf-output-style=JSON"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )

    try:
        result = json.loads(proc.stdout.decode())
    except:
        sys.stderr.write(proc.stderr.decode())
        sys.stderr.flush()
        exit(-1)

    for elf_section_name in elf_sections:
        section = list(filter(
            lambda i: i["Section"]["Name"]["Name"] == elf_section_name,
            result[0]["Sections"]
        ))[0]["Section"]
        out[elf_section_name] = section["Size"]

    return out

def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <shift value>\n\n\t(note that  0 <= shift value <= 10)")
        return 1

    try:
        shift = int(sys.argv[1])
        if not 0 <= shift <= 10:
            raise ValueError
    except ValueError:
        print(f"shift value of {sys.argv[1]} is invalid")
        return 1

    q = queue.Queue()
    threads = [Thread(target=get_coremark, args=(i, q, shift)) for i in triples]

    data = {}

    for triple in triples:
        data[triple] = get_sizes(triple)

    # start all threads and wait for them to join
    for i in threads: i.start()
    for i in threads: i.join()
    while not q.empty():
        triple, score, t = q.get()
        data[triple]["coremark"] = score
        data[triple]["coremark_time"] = t
        data[triple]["qemu_info"] = qemu_info[triple]

    data["triples"] = triples
    data["elf_sections"] = elf_sections

    # this is because subprocess.run sets stdout to nonblocking,
    # which occasionally causes the following print statement to
    # throw an OSError, so we set it back to blocking manually
    os.set_blocking(sys.stdout.fileno(), True)

    print(json.dumps(data, indent=2))
    return 0

if __name__ == "__main__":
    exit(main())