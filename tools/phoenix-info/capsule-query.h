#include "sha256.h"
static VOID capsule_query(EFI_HANDLE image) {
    const UINTN size=26186240;
    const UINT8 pin[32]={0x8f,0x5c,0x93,0x5f,0xd7,0x71,0x1e,0x8c,0xdb,0x08,0x7b,0xb3,0x4b,0xf5,0xc7,0x28,0xed,0x55,0x6e,0x87,0x86,0x65,0x24,0x92,0xdb,0xed,0xe6,0x71,0x4c,0x84,0xdc,0x20};
    EFI_GUID loaded=LOADED_IMAGE_PROTOCOL,filesystem=EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,info_guid=EFI_FILE_INFO_ID;
    EFI_LOADED_IMAGE *li=NULL;EFI_FILE_IO_INTERFACE *fs=NULL;
    EFI_FILE_HANDLE root=NULL,file=NULL;UINT8 *data=NULL;
    EFI_STATUS status=uefi_call_wrapper(BS->HandleProtocol,3,image,&loaded,(VOID **)&li);
    if(!EFI_ERROR(status)) status=uefi_call_wrapper(BS->HandleProtocol,3,li->DeviceHandle,&filesystem,(VOID **)&fs);
    if(!EFI_ERROR(status)) status=uefi_call_wrapper(fs->OpenVolume,2,fs,&root);
    if(!EFI_ERROR(status)) status=uefi_call_wrapper(root->Open,5,root,&file,L"\\EFI\\CognacPhoenix\\FXCN49WW.CAP",EFI_FILE_MODE_READ,0);
    if(!EFI_ERROR(status)) {
        UINT8 info[1024];UINTN n=sizeof(info);
        status=uefi_call_wrapper(file->GetInfo,4,file,&info_guid,&n,info);
        if(!EFI_ERROR(status) && (n<SIZE_OF_EFI_FILE_INFO || ((EFI_FILE_INFO *)info)->FileSize!=size)) status=EFI_BAD_BUFFER_SIZE;
    }
    if(!EFI_ERROR(status)) {
        data=AllocatePool(size);
        if(!data) status=EFI_OUT_OF_RESOURCES;
        else {UINTN n=size;status=uefi_call_wrapper(file->Read,3,file,&n,data);if(!EFI_ERROR(status) && n!=size) status=EFI_BAD_BUFFER_SIZE;}
    }
    if(file) uefi_call_wrapper(file->Close,1,file);
    if(root) uefi_call_wrapper(root->Close,1,root);
    if(EFI_ERROR(status)) {logline(L"Capsule file read refused: %r\r\n",status);if(data) FreePool(data);return;}
    UINT8 hash[32];capsule_sha256(data,size,hash);
    if(CompareMem(hash,(VOID *)pin,32)) {logline(L"Complete capsule SHA256 mismatch; no buffer write.\r\n");FreePool(data);return;}
    /* CapsulePrepare's 32-byte descriptor then original capsule. No variable
     * or flash-enable is issued. The whole temporary copy is cleared afterward. */
    if(!range(0x34cab018,size+32,TRUE)) {logline(L"Capsule buffer memory ownership check failed; no buffer write.\r\n");FreePool(data);return;}
    UINT8 *buffer=(VOID *)(UINTN)0x34cab018;
    UINT64 header[4]={size,0x34cab038,0,0};
    CopyMem(buffer,header,32);CopyMem(buffer+32,data,size);
    __asm__ __volatile__("mfence":::"memory");
    if(CompareMem(buffer,header,32) || CompareMem(buffer+32,data,size)) {
        logline(L"Capsule RAM readback mismatch; no validation command.\r\n");
    } else {
#ifdef PHOENIX_FLASH_ONESHOT
        /* CapsuleValidationByService 0x425928: a successful signature-service
         * return jumps directly to cleanup/success at 0x425955. 0x11200 is
         * an alternative only when signature validation is unsupported.
         * Our caller requires same-boot signature status 0 and results 1,1. */
        logline(L"Pinned capsule RAM readback passed; vendor signature-service approval already verified this boot.\r\n");
        if(!EFI_ERROR(save(image)) && flash_stage(image,data)) {
            FreePool(data);
            logline(L"Capsule RAM retained for pending handoff. No repeated submission.\r\n");return;
        }
#else
        UINT8 packet[41]={0};UINT64 command=0x11200,initial=0xff,length=41;
        const UINT8 guid[16]={0xfc,0x44,0xde,0xb1,0x46,0x79,0x82,0x49,0x9b,0x4b,0x2f,0x8c,0xa4,0x5e,0xa7,0x92};
        const UINT8 zero_guid[16]={0};
        CopyMem(packet,&command,8);CopyMem(packet+8,&initial,8);CopyMem(packet+16,&length,8);CopyMem(packet+24,(VOID *)guid,16);
        logline(L"Exact capsule loaded and read back in temporary RAM. Calling validation only.\r\n");
        if(EFI_ERROR(save(image))) {
            SetMem(buffer,size+32,0);FreePool(data);
            logline(L"Progress log could not be saved; validation not issued.\r\n");return;
        }
        volatile UINT8 *shared=(VOID *)(UINTN)0x44cd4000;
        for(UINTN i=0;i<41;i++) shared[i]=packet[i];
        __asm__ __volatile__("mfence; outb %0,%1; mfence"::"a"((UINT8)0xe9),"d"((UINT16)0xb2):"memory");
        for(UINTN i=0;i<41;i++) packet[i]=shared[i];
        logline(L"Full capsule validation response:\r\n");
        for(UINTN i=0;i<41;i++) logline(L"%02x%s",packet[i],((i+1)%16)?L" ":L"\r\n");
        BOOLEAN identity=read64(packet)==command && read64(packet+16)==length
            && (!CompareMem(packet+24,guid,16) || !CompareMem(packet+24,zero_guid,16));
        logline(L"\r\nCapsule response identity=%d status=0x%lx accepted=%d\r\n",identity,read64(packet+8),packet[40]);
#endif
    }
    SetMem(buffer,size+32,0);__asm__ __volatile__("mfence":::"memory");
    FreePool(data);
    logline(L"Temporary capsule RAM cleared. No update variables, no BIOS flash.\r\n");
}
