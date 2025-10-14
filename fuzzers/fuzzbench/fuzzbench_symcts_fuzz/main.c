#include <sys/types.h>
#include <stdint.h>
#include <stdio.h>

extern "C" int LLVMFuzzerTestOneInput(char* data, size_t sz) {
    // simple bare-bones ELF parser to simulate readelf
    if (sz < 36) return 0;  // Not enough data for ELF header

    // Check ELF magic number
    if (data[0] != 0x7f || data[1] != 'E' || data[2] != 'L' || data[3] != 'F') {
        return 0;
    }

    // Read ELF header fields (simplified)
    uint32_t entry = *(uint32_t*)(data + 24);
    uint32_t phoff = *(uint32_t*)(data + 28);
    uint32_t shoff = *(uint32_t*)(data + 32);

    // Simulate readelf output
    printf("ELF Header:\n");
    printf("  Entry point: 0x%x\n", entry);
    printf("  Program header offset: 0x%x\n", phoff);
    printf("  Section header offset: 0x%x\n", shoff);

    // validate phoff and shoff
    if (phoff < 36 || shoff < 36) return 0;
    if (phoff >= sz || shoff >= sz) return 0;

    for (int i = 0; i < 5; i++) {
        if (sz < phoff + i * 32 + 32) break;  // Not enough data for program header
        
        printf("  Program header %d:\n", i);
        printf("    Type: 0x%x\n", *(uint32_t*)(data + phoff + i * 32));
        printf("    Offset: 0x%x\n", *(uint32_t*)(data + phoff + i * 32 + 8));
        printf("    Virtual address: 0x%x\n", *(uint32_t*)(data + phoff + i * 32 + 12));
        printf("    Physical address: 0x%x\n", *(uint32_t*)(data + phoff + i * 32 + 16));
        printf("    File size: 0x%x\n", *(uint32_t*)(data + phoff + i * 32 + 20));
        printf("    Memory size: 0x%x\n", *(uint32_t*)(data + phoff + i * 32 + 24));
    }

    printf("Section Headers:\n");
    for (int i = 0; i < 5; i++) {
        if (sz < shoff + i * 40 + 40) break;  // Not enough data for section header
        printf("  Section header %d:\n", i);
        printf("    Type: 0x%x\n", *(uint32_t*)(data + shoff + i * 40));
        printf("    Offset: 0x%x\n", *(uint32_t*)(data + shoff + i * 40 + 8));
        printf("    Virtual address: 0x%x\n", *(uint32_t*)(data + shoff + i * 40 + 12));
        printf("    Physical address: 0x%x\n", *(uint32_t*)(data + shoff + i * 40 + 16));
        printf("    File size: 0x%x\n", *(uint32_t*)(data + shoff + i * 40 + 20));
        printf("    Memory size: 0x%x\n", *(uint32_t*)(data + shoff + i * 40 + 24));
    }

    printf("String Table:\n");
    for (int i = 0; i < 5; i++) {
        if (sz < shoff + i * 40 + 28) break;  // Not enough data for string table
        printf("  String %d: %s\n", i, (char*)(data + shoff + i * 40 + 28));
    }

    return 0;
}
