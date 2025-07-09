#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

int LLVMFuzzerTestOneInput(const uint8_t *Data, size_t Size) {
  printf("Size: %zu\n", Size);
  if (Size >= 8 && *(uint32_t *)Data == 0xaabbccdd) { abort(); }
  printf("Data: ");
  char buf[8] = {'a', 'b', 'c', 'd'};

  if (memcmp(Data, buf, 4) == 0) { abort(); }
  printf("Finished processing input.\n");
  return 0;
}

/*
int main() {

  char buf [10] = {0};
  LLVMFuzzerTestOneInput(buf, 10);

}*/
