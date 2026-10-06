/* Exact traced ServiceCapsuleSignatureVerify request. No flash-enable,
 * capsule-buffer writes, update variable, erase or reset command. */
#include "signature-data.h"
static BOOLEAN signature_query(VOID) {
    EFI_PHYSICAL_ADDRESS page=0xffffffff;
    EFI_STATUS status=uefi_call_wrapper(BS->AllocatePages,4,AllocateMaxAddress,
        EfiRuntimeServicesData,(UINTN)1,&page);
    if(EFI_ERROR(status)) {logline(L"Signature RAM allocation failed: %r\r\n",status);return FALSE;}
    UINT8 *ram=(VOID *)(UINTN)page;
    SetMem(ram,4096,0);
    CopyMem(ram,(VOID *)verified_hash,32);
    CopyMem(ram+33,(VOID *)verified_signature,256);
    UINT64 packet[12]={0x13000,0xff,96};
    const UINT8 guid[16]={0xfc,0x44,0xde,0xb1,0x46,0x79,0x82,0x49,0x9b,0x4b,0x2f,0x8c,0xa4,0x5e,0xa7,0x92};
    const UINT8 zero_guid[16]={0};
    CopyMem((UINT8 *)packet+24,(VOID *)guid,16);
    packet[5]=32;packet[6]=page;packet[7]=256;packet[8]=page+33;packet[9]=3;
    volatile UINT8 *shared=(VOID *)(UINTN)0x44cd4000;
    for(UINTN i=0;i<96;i++) shared[i]=((UINT8 *)packet)[i];
    __asm__ __volatile__("mfence; outb %0,%1; mfence"::"a"((UINT8)0xe9),"d"((UINT16)0xb2):"memory");
    for(UINTN i=0;i<96;i++) ((UINT8 *)packet)[i]=shared[i];
    logline(L"\r\nSignature validation raw response:\r\n");
    for(UINTN i=0;i<96;i++) logline(L"%02x%s",((UINT8 *)packet)[i],((i+1)%16)?L" ":L"\r\n");
    BOOLEAN identity=packet[0]==0x13000 && packet[2]==96
       && (!CompareMem((UINT8 *)packet+24,guid,16) || !CompareMem((UINT8 *)packet+24,zero_guid,16));
    if(!identity)
        logline(L"Signature response identity mismatch; unverified.\r\n");
    else logline(L"Signature firmware status=0x%lx result fields=0x%lx,0x%lx\r\n",
        packet[1],packet[10],packet[11]);
    logline(L"No BIOS flash performed. Full capsule/model validation still required.\r\n");
    SetMem(ram,4096,0);
    uefi_call_wrapper(BS->FreePages,2,page,(UINTN)1);
    return identity && packet[1]==0 && packet[10]==1 && packet[11]==1;
}
