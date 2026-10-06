/* Exact-platform vendor RAM-capsule handoff. No raw SPI/erase operations.
 * This is compiled only into the explicitly authorized one-shot variant. */
static const UINT8 flash_identity[16]={0xfc,0x44,0xde,0xb1,0x46,0x79,0x82,0x49,0x9b,0x4b,0x2f,0x8c,0xa4,0x5e,0xa7,0x92};
static EFI_GUID capsule_vendor={0x711c703f,0xc285,0x4b10,{0xa3,0xb0,0x36,0xec,0xbd,0x3c,0x8b,0xe2}};
static EFI_GUID capsule_info={0x79dfd2ed,0x281f,0x425f,{0x97,0xa8,0x7a,0x88,0x30,0xf6,0x2f,0x6e}};
#ifdef PHOENIX_FLASH_TEST
static VOID flash_exchange(UINT8 *packet,UINTN size);
#else
static VOID flash_exchange(UINT8 *packet,UINTN size) {
    volatile UINT8 *shared=(VOID *)(UINTN)0x44cd4000;
    for(UINTN i=0;i<size;i++) shared[i]=packet[i];
    __asm__ __volatile__("mfence; outb %0,%1; mfence"::"a"((UINT8)0xe9),"d"((UINT16)0xb2):"memory");
    for(UINTN i=0;i<size;i++) packet[i]=shared[i];
}
#endif
static BOOLEAN tdk_identity(UINT8 *packet,UINT64 command,UINT64 length) {
    const UINT8 zero[16]={0};
    return read64(packet)==command && read64(packet+16)==length
        && (!CompareMem(packet+24,(VOID *)flash_identity,16) || !CompareMem(packet+24,(VOID *)zero,16));
}
static BOOLEAN flash_switch(BOOLEAN enable) {
    UINT8 packet[113]={0};UINT64 command=9,initial=0xff;
    CopyMem(packet,&command,8);CopyMem(packet+80,&initial,8);
    CopyMem(packet+96,(VOID *)flash_identity,16);packet[112]=enable;
    flash_exchange(packet,sizeof(packet));
    logline(L"Vendor flash switch enable=%d command=0x%lx status=0x%lx\r\n",enable,read64(packet),read64(packet+80));
    return read64(packet)==9 && read64(packet+80)==0;
}
static BOOLEAN nv_completed(VOID) {
    for(UINTN i=0;i<12000;i++) {
        UINT64 packet[5]={0x13300,0xff,40};CopyMem((UINT8 *)packet+24,(VOID *)flash_identity,16);
        flash_exchange((UINT8 *)packet,40);
        if(!tdk_identity((UINT8 *)packet,0x13300,40)) return FALSE;
        if(packet[1]==0) return TRUE;
        if(packet[1]!=6) {logline(L"NVS completion status=0x%lx\r\n",packet[1]);return FALSE;}
        uefi_call_wrapper(BS->Stall,1,(UINTN)5000);
    }
    logline(L"NVS completion timed out. No automatic retry of variable write.\r\n");return FALSE;
}
static BOOLEAN vendor_variable(CHAR16 *name,EFI_GUID *guid,UINT8 *value,UINTN size,EFI_PHYSICAL_ADDRESS page) {
    UINT8 packet[256]={0};UINTN name_bytes=(StrLen(name)+1)*2;
    if(name_bytes+80>sizeof(packet) || size>4096) return FALSE;
    UINT64 command=0x10600,initial=0xff,length=80,attributes=size?7:0,data_size=size,address=size?page:0;
    CopyMem(packet,&command,8);CopyMem(packet+8,&initial,8);CopyMem(packet+16,&length,8);
    CopyMem(packet+24,(VOID *)flash_identity,16);CopyMem(packet+40,guid,16);
    CopyMem(packet+56,&attributes,8);CopyMem(packet+64,&data_size,8);CopyMem(packet+72,&address,8);
    CopyMem(packet+80,name,name_bytes);
    if(size) CopyMem((VOID *)(UINTN)page,value,size);
    flash_exchange(packet,80+name_bytes);
    logline(L"Vendor variable %s bytes=%ld status=0x%lx\r\n",name,(UINT64)size,read64(packet+8));
    return tdk_identity(packet,command,length) && read64(packet+8)==0 && nv_completed();
}
static BOOLEAN variable_matches(CHAR16 *name,EFI_GUID *guid,UINT8 *value,UINTN size) {
    UINT8 actual[32];UINTN n=sizeof(actual);UINT32 attributes=0;
    EFI_STATUS status=uefi_call_wrapper(RT->GetVariable,5,name,guid,&attributes,&n,actual);
    return !EFI_ERROR(status) && n==size && attributes==7 && !CompareMem(actual,value,size);
}
static BOOLEAN absent_variable(CHAR16 *name,EFI_GUID *guid) {
    UINTN size=0;EFI_STATUS status=uefi_call_wrapper(RT->GetVariable,5,name,guid,NULL,&size,NULL);
    return status==EFI_NOT_FOUND;
}
static BOOLEAN flash_stage(EFI_HANDLE image,UINT8 *capsule) {
    if(!absent_variable(L"CapsuleUpdateData",&capsule_vendor)
       || !absent_variable(L"CapsuleGuidData",&capsule_info)) {
        logline(L"Existing vendor update variables; staging refused.\r\n");return FALSE;
    }
    EFI_PHYSICAL_ADDRESS page=0xffffffff;
    EFI_STATUS status=uefi_call_wrapper(BS->AllocatePages,4,AllocateMaxAddress,EfiRuntimeServicesData,(UINTN)1,&page);
    if(EFI_ERROR(status)) return FALSE;
    SetMem((VOID *)(UINTN)page,4096,0);
    /* Read-only flash-channel detection, traced ServiceChannelFlashCommDetect. */
    UINT8 detect[113]={0};UINT64 command=5,one=1,top=0xfffffff0,initial=0xff;
    UINT64 second=page+0x400,third=page+0x800;
    CopyMem(detect,&command,8);CopyMem(detect+8,&one,8);CopyMem(detect+16,&top,8);
    CopyMem(detect+48,&page,8);CopyMem(detect+72,&second,8);CopyMem(detect+88,&third,8);
    CopyMem(detect+80,&initial,8);CopyMem(detect+96,(VOID *)flash_identity,16);
    flash_exchange(detect,sizeof(detect));
    logline(L"Flash channel detection command=0x%lx status=0x%lx\r\n",read64(detect),read64(detect+80));
    if(read64(detect)!=5 || read64(detect+80)!=0 || EFI_ERROR(save(image))) {
        uefi_call_wrapper(BS->FreePages,2,page,(UINTN)1);return FALSE;
    }
    logline(L"Pinned capsule and vendor signature accepted. Beginning authorized vendor handoff.\r\n");
    if(EFI_ERROR(save(image))) {uefi_call_wrapper(BS->FreePages,2,page,(UINTN)1);return FALSE;}
    BOOLEAN enabled=flash_switch(TRUE);
    UINT64 capsule_address=0x34cab018;
    BOOLEAN first=FALSE,second_set=FALSE;
    if(enabled) {
        first=vendor_variable(L"CapsuleUpdateData",&capsule_vendor,(UINT8 *)&capsule_address,8,page);
        if(first && variable_matches(L"CapsuleUpdateData",&capsule_vendor,(UINT8 *)&capsule_address,8))
            second_set=vendor_variable(L"CapsuleGuidData",&capsule_info,capsule,16,page);
    }
    BOOLEAN ready=first && second_set
        && variable_matches(L"CapsuleUpdateData",&capsule_vendor,(UINT8 *)&capsule_address,8)
        && variable_matches(L"CapsuleGuidData",&capsule_info,capsule,16);
    BOOLEAN cleanup_uncertain=FALSE;
    if(!ready && enabled) {
        logline(L"Handoff incomplete; removing this attempt's trigger variables.\r\n");
        vendor_variable(L"CapsuleUpdateData",&capsule_vendor,NULL,0,page);
        vendor_variable(L"CapsuleGuidData",&capsule_info,NULL,0,page);
        if(!absent_variable(L"CapsuleUpdateData",&capsule_vendor) || !absent_variable(L"CapsuleGuidData",&capsule_info)) {
            logline(L"Cleanup uncertain; automatic reset blocked.\r\n");
            cleanup_uncertain=TRUE;
        }
    }
    /* Attempt disable even after an ambiguous enable response. */
    BOOLEAN disabled=flash_switch(FALSE);
    SetMem((VOID *)(UINTN)page,4096,0);uefi_call_wrapper(BS->FreePages,2,page,(UINTN)1);
    if(ready && disabled) {
        logline(L"Vendor handoff readback verified; flash switch disabled. Warm resetting for firmware update.\r\n");
        if(EFI_ERROR(save(image))) {logline(L"Final journal failed; automatic reset blocked.\r\n");return TRUE;}
        __asm__ __volatile__("mfence":::"memory");
        uefi_call_wrapper(RT->ResetSystem,4,EfiResetWarm,EFI_SUCCESS,(UINTN)0,NULL);
        logline(L"Reset returned unexpectedly; update remains staged.\r\n");return TRUE;
    }
    return ready || cleanup_uncertain;
}
