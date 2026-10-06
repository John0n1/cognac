#include <efi.h>
#include <efilib.h>

/* Host process only: exercise the actual linked GNU-EFI implementation with
 * unaligned input, the same operation used by the ACPI integer readers. */
int main(void) {
    UINT8 input[9]={0xa5,0x78,0x56,0x34,0x12,0x11,0x22,0x33,0x44};
    UINT32 value=0;
    UINT8 output[9]={0};
    CopyMem(&value,input+1,4);
    if(value!=0x12345678) return 1;
    CopyMem(output,input,sizeof(input));
    if(CompareMem(output,input,sizeof(input))) return 2;
    SetMem(output,sizeof(output),0x5a);
    for(UINTN i=0;i<sizeof(output);i++) if(output[i]!=0x5a) return 3;
    return 0;
}
