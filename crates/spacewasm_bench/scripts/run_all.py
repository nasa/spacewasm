import queue
import json
import subprocess
from threading import Thread

triples = [
    "riscv32i-unknown-none-elf",
    "riscv64gc-unknown-none-elf",
    "thumbv7m-none-eabi",
    "thumbv7em-none-eabihf",
]

elf_sections = [
    ".text",
    ".rodata",
    ".data",
    ".bss"
]

def get_coremark(triple, q):
    command = ["cargo", "readobj", "-q", "--release", "-p", "spacewasm_bench", "--target", triple, "--", "--sections"]
    proc = subprocess.run(
        command,
        capture_output=True
    )

    output = proc.stdout.decode()

    q.put((triple, output))

def get_sizes(triple):
    out = {}

    command = ["cargo", "readobj", "-q", "--release", "-p", "spacewasm_bench", "--target", triple, "--", "--sections"]
    proc = subprocess.run(
        command,
        capture_output=True
    )

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


def main():
    q = queue.Queue()
    threads = [Thread(target=get_sizes, args=(i, q)) for i in triples]
    for i in threads: i.start()

    data = {}
    old_data = {'riscv32i-unknown-none-elf': {'.text': 435, '.rodata': 5093, '.data': 0, '.bss': 130388}, 'riscv64gc-unknown-none-elf': {'.text': 9800, '.rodata': 52800, '.data': 0, '.bss': 1004224}, 'thumbv7m-none-eabi': {'.text': 2252, '.rodata': 49228, '.data': 0, '.bss': 1050404}, 'thumbv7em-none-eabihf': {'.text': 248, '.rodata': 49528, '.data': 0, '.bss': 10043304}}
    for triple in triples:
        data[triple] = get_sizes(triple)

    print("# ELF Section Sizes")
    print("||" + "|".join(map(lambda i: f"`{i}`", triples)) + "|")
    print("|--:|" + ":-:|" * len(triples))
    for section_name in elf_sections:
        print(f"|`{section_name}`|", end="")
        for triple in triples:
            size = data[triple][section_name]
            old_size = old_data[triple][section_name]

            color = "normalcolor"
            change_text = ""

            if old_size != size:
                pct_change = round(100 * (size - old_size) / old_size, 1)
                if pct_change > 0:
                    color = "red"
                    change_text = r"\ (\uparrow " + str(abs(pct_change)) + r" \\%\ \text{from}\ " + str(old_size) + ")"
                else:
                    color = "green"
                    change_text = r"\ (\downarrow " + str(abs(pct_change)) + r" \\%\ \text{from}\ " + str(old_size) + ")"

            print(r"$${\color{" + color + r"}{" + str(size) + change_text + r"}}$$|", end="")
        print()
    print("<details><summary><i>view detailed section sizes...</i></summary>\n")
    for triple in triples:
        print(f"### `{triple}`")
        print(f"```\n{data[triple]["details"]}\n```\n")
    print("</details>\n")

    print("# Coremark Scores")
    for i in threads: i.join()
    while q.not_empty:
        print(q.get())


    return 0

if __name__ == "__main__":
    exit(main())