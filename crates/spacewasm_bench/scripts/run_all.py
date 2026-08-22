import queue
import json
import math
import subprocess
import sys
import os
import time
from threading import Thread

qemu_commands = {
    "riscv32i-unknown-none-elf": "qemu-system-riscv32 -icount shift=0 -bios none -cpu rv32 -machine virt -m 256M -nographic -semihosting-config enable=on,target=native -kernel",
    "riscv64gc-unknown-none-elf": "qemu-system-riscv64 -icount shift=0 -bios none -cpu rv64 -machine virt -m 256M -nographic -semihosting-config enable=on,target=native -kernel",
    "thumbv7m-none-eabi": "qemu-system-arm -icount shift=0 -cpu cortex-m4 -machine netduinoplus2 -nographic -semihosting-config enable=on,target=native -kernel",
    "thumbv7em-none-eabihf": "qemu-system-arm -icount shift=0 -cpu cortex-m4 -machine netduinoplus2 -nographic -semihosting-config enable=on,target=native -kernel",
}
qemu_info = {
    "riscv32i-unknown-none-elf": "`rv32` / `virt` @ 1GHz",
    "riscv64gc-unknown-none-elf": "`rv64` / `virt` @ 1GHz",
    "thumbv7m-none-eabi": "`cortex-m4` / `netduinoplus2` @ 1GHz",
    "thumbv7em-none-eabihf": "`cortex-m4` / `netduinoplus2` @ 1GHz",
}
triples = list(qemu_commands.keys())
elf_sections = [".text", ".rodata", ".data", ".bss"]

def get_coremark(triple, q):
    # print("starting coremark for", f"../../target/{triple}/release/spacewasm_bench")
    command = [*qemu_commands[triple].split(" "), f"../../target/{triple}/release/spacewasm_bench"]

    start_time = time.time()
    proc = subprocess.run(command, capture_output=True)
    full_time = time.time() - start_time

    output = float(proc.stdout.decode())

    q.put((triple, output, full_time))

def get_sizes(triple):
    out = {}

    command = ["cargo", "readobj", "-q", "--release", "-p", "spacewasm_bench", "--target", triple, "--", "--sections"]
    proc = subprocess.run(command, capture_output=True)

    out["details"] = proc.stdout.decode()

    proc = subprocess.run(
        [*command, "--elf-output-style=JSON"],
        capture_output=True
    )
    result = json.loads(proc.stdout.decode())


    for elf_section_name in elf_sections:
        section = list(filter(lambda i: i["Section"]["Name"]["Name"] == elf_section_name, result[0]["Sections"]))[0]["Section"]

        out[elf_section_name] = section["Size"]

    return out

def colorize(new, old, lower_better):
    color = "normalcolor"
    change_text = ""

    if old != new:
        if old == 0:
            pct_change = math.inf if new > 0 else -math.inf
        else:
            pct_change = round(100 * (new - old) / old, 1)
        if pct_change > 0:
            color = "red" if lower_better else "green"
            change_text = r"(↑" + str(pct_change) + r"% from " + str(old) + ")"
        else:
            color = "green" if lower_better else "red"
            change_text = r"(↓" + str(-pct_change) + r"% from " + str(old) + ")"

    return r"$${\color{" + color + r"}{" + str(new) + r"}}$$<br>"+change_text

def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <save.json>")
        return 1

    old_data = {}
    if os.path.exists(sys.argv[1]):
        old_data = json.load(open(sys.argv[1], "r+"))

    q = queue.Queue()
    threads = [Thread(target=get_coremark, args=(i, q)) for i in triples]

    data = {}

    for triple in triples:
        data[triple] = get_sizes(triple)

    print("# ELF Section Sizes")
    print("||" + "|".join(map(lambda i: f"`{i}`", elf_sections)) + "|")
    print("|--:|" + ":-:|" * len(triples))
    for triple in triples:
        print(f"|**`{triple}`**|", end="")
        for section_name in elf_sections:
            size = data[triple][section_name]
            old_size = old_data.get(triple, {}).get(section_name, size)

            print(colorize(size, old_size, True), end="|")
        print()
    print("<details><summary><i>view detailed section sizes...</i></summary>\n")
    for triple in triples:
        print(f"### `{triple}`")
        print(f"```\n{data[triple]["details"]}\n```\n")
    print("</details>\n")

    print("# Coremark Scores")

    sys.stdout.flush()

    for i in threads: i.start()
    for i in threads: i.join()
    while not q.empty():
        triple, score, t = q.get()
        data[triple]["coremark"] = score
        data[triple]["coremark_time"] = t

    print(f"||QEMU CPU / Board|Coremark Score|Host Time|")
    print(f"|--:|:-:|:-:|:-:|")
    for triple in triples:
        score = data[triple]["coremark"]
        t = data[triple]["coremark_time"]
        old_score = old_data.get(triple, {}).get("coremark", score)
        print(f"|**`{triple}`**|{qemu_info[triple]}|{colorize(score, old_score, False)}|{round(t, 2)} s|")
    print()

    with open(sys.argv[1], "w+") as f:
        json.dump(data, f, indent=2)

    return 0

if __name__ == "__main__":
    exit(main())