#include <efi.h>
#include <efilib.h>
#include "tables.h"

/* Experimental, exact-platform GET_INFO probe. No capsule, flash enable,
 * variable write, erase, reboot or update command is included. It writes
 * a GET_INFO request, then fixed capsule-buffer and hash-option queries
 * into firmware-owned RAM, only after the operator presses I. */
static EFI_MEMORY_DESCRIPTOR *map;
static UINTN map_size, descriptor_size;
static CHAR8 logbuf[8192];
static UINTN loglen;
static VOID logline(CHAR16 *fmt, ...) {
    CHAR16 text[512];va_list args;va_start(args,fmt);
    VSPrint(text,sizeof(text),fmt,args);va_end(args);Print(L"%s",text);
    for(UINTN i=0;text[i] && loglen+1<sizeof(logbuf);i++)
        logbuf[loglen++]=text[i]<128?(CHAR8)text[i]:'?';
}
static BOOLEAN range(UINT64 address, UINTN length, BOOLEAN shared) {
    if(!map || descriptor_size<sizeof(EFI_MEMORY_DESCRIPTOR) || !map_size
       || map_size%descriptor_size || map_size/descriptor_size>16384) {
        logline(L"Invalid memory-map dimensions\r\n");return FALSE;
    }
    if(!address || !length || address+length<address) { logline(L"Invalid range address=0x%lx size=%ld\r\n",address,(UINT64)length); return FALSE; }
    for(UINTN i=0;i<map_size;i+=descriptor_size) {
        EFI_MEMORY_DESCRIPTOR *d=(VOID *)((UINT8 *)map+i);
        if(d->NumberOfPages>(~(UINT64)0)/4096) continue;
        UINT64 end=d->PhysicalStart+d->NumberOfPages*4096;
        if(end<d->PhysicalStart) continue;
        if(address>=d->PhysicalStart && address+length<=end) {
            if(shared) {
                logline(L"Shared region memory type=%d start=0x%lx end=0x%lx\r\n",d->Type,d->PhysicalStart,end);
                /* Phoenix's independently queried exact capsule buffer lies
                 * in runtime-services data. This exception applies only to
                 * the pinned image's precise accessed range, never to the
                 * SMI request area or arbitrary runtime memory. */
                if(address==0x34cab018 && length==26186272
                   && d->Type==EfiRuntimeServicesData) return TRUE;
                return d->Type==EfiReservedMemoryType || d->Type==EfiACPIMemoryNVS;
            }
            return d->Type!=EfiMemoryMappedIO && d->Type!=EfiMemoryMappedIOPortSpace
                && d->Type!=EfiUnusableMemory;
        }
    }
    logline(L"Range absent from UEFI map: address=0x%lx size=%ld\r\n",address,(UINT64)length);
    return FALSE;
}
static UINT32 read32(const VOID *p) {UINT32 v;CopyMem(&v,(VOID *)p,4);return v;}
static UINT64 read64(const VOID *p) {UINT64 v;CopyMem(&v,(VOID *)p,8);return v;}
static BOOLEAN checksum(const UINT8 *p, UINTN n) {
    UINT8 sum=0;for(UINTN i=0;i<n;i++) sum+=p[i];return sum==0;
}
static BOOLEAN platform(EFI_SYSTEM_TABLE *st) {
    EFI_GUID acpi={0x8868e871,0xe4f1,0x11d3,{0xbc,0x22,0x00,0x80,0xc7,0x3c,0x88,0x81}};
    UINT8 *rsdp=NULL;
    for(UINTN i=0;i<st->NumberOfTableEntries;i++)
        if(!CompareMem(&st->ConfigurationTable[i].VendorGuid,&acpi,16)) rsdp=st->ConfigurationTable[i].VendorTable;
    logline(L"RSDP address=0x%lx\r\n",(UINT64)(UINTN)rsdp);
    if(!range((UINTN)rsdp,36,FALSE)) {logline(L"RSDP memory check failed\r\n");return FALSE;}
    logline(L"RSDP revision=%d length=%d checksum20=%d checksum36=%d\r\n",rsdp[15],read32(rsdp+20),checksum(rsdp,20),checksum(rsdp,36));
    if(CompareMem(rsdp,"RSD PTR ",8) || rsdp[15]<2 || read32(rsdp+20)!=36 || !checksum(rsdp,20) || !checksum(rsdp,36)) {logline(L"RSDP structure check failed\r\n");return FALSE;}
    UINT64 address=read64(rsdp+24);
    logline(L"XSDT address=0x%lx\r\n",address);
    if(!range(address,36,FALSE)) {logline(L"XSDT header memory check failed\r\n");return FALSE;}
    UINT8 *xsdt=(VOID *)(UINTN)address;UINTN len=read32(xsdt+4);
    if(CompareMem(xsdt,"XSDT",4) || len<36 || len>65536 || (len-36)%8
       || !range(address,len,FALSE) || !checksum(xsdt,len)) {logline(L"XSDT structure check failed, length=%ld\r\n",(UINT64)len);return FALSE;}
    BOOLEAN u=FALSE,b=FALSE,f=FALSE;
    for(UINTN i=36;i<len;i+=8) {
        UINT64 a=read64(xsdt+i);
        logline(L"Table index=%ld address=0x%lx\r\n",(UINT64)((i-36)/8),a);
        if(!range(a,36,FALSE)) {logline(L"Table header memory check failed\r\n");return FALSE;}
        UINT8 *t=(VOID *)(UINTN)a;UINTN n=read32(t+4);
        logline(L"Table signature=%c%c%c%c length=%ld\r\n",t[0],t[1],t[2],t[3],(UINT64)n);
        if(n<36 || n>1048576 || !range(a,n,FALSE)) {logline(L"Table length/memory check failed\r\n");return FALSE;}
        if(!CompareMem(t,"UEFI",4) && n==sizeof(expected_uefi) && !CompareMem(t,expected_uefi,n)) u=TRUE;
        if(!CompareMem(t,"BATB",4) && n==sizeof(expected_batb) && !CompareMem(t,expected_batb,n)) b=TRUE;
        if(!CompareMem(t,"FACP",4) && n==sizeof(expected_facp) && !CompareMem(t,expected_facp,n)) f=TRUE;
    }
    logline(L"Exact table matches UEFI=%d BATB=%d FACP=%d\r\n",u,b,f);
    BOOLEAN shared_ok=range(0x44cd4000,512,TRUE);
    return u && b && f && shared_ok;
}
/* Fixed, query-only requests traced from ServiceGetCapsuleBuffer and
 * ServiceCapsuleGetHashOptions. No caller-supplied opcode or payload. */
static BOOLEAN capability_query(BOOLEAN buffer_query) {
    UINT64 packet[7]={0};
    UINTN length=buffer_query?56:48;
    packet[0]=buffer_query?0x10800:0x12900;packet[1]=0xff;packet[2]=length;
    const UINT8 guid[16]={0xfc,0x44,0xde,0xb1,0x46,0x79,0x82,0x49,0x9b,0x4b,0x2f,0x8c,0xa4,0x5e,0xa7,0x92};
    const UINT8 zero_guid[16]={0};
    CopyMem((UINT8 *)packet+24,(VOID *)guid,16);
    volatile UINT8 *shared=(VOID *)(UINTN)0x44cd4000;
    for(UINTN i=0;i<length;i++) shared[i]=((UINT8 *)packet)[i];
    __asm__ __volatile__("mfence; outb %0,%1; mfence"::"a"((UINT8)0xe9),"d"((UINT16)0xb2):"memory");
    for(UINTN i=0;i<length;i++) ((UINT8 *)packet)[i]=shared[i];
    logline(L"\r\nCapability query 0x%lx raw response:\r\n",buffer_query?(UINT64)0x10800:(UINT64)0x12900);
    for(UINTN i=0;i<length;i++) logline(L"%02x%s",((UINT8 *)packet)[i],((i+1)%16)?L" ":L"\r\n");
    logline(L"\r\nFirmware status=0x%lx\r\n",packet[1]);
    if(packet[0]!=(buffer_query?0x10800:0x12900) || packet[2]!=length || packet[1]!=0
       || (CompareMem((UINT8 *)packet+24,guid,16) && CompareMem((UINT8 *)packet+24,zero_guid,16))) {
        logline(L"Capability response not accepted; stopping query sequence.\r\n");return FALSE;
    }
    if(buffer_query) {
        if(!packet[5] || !packet[6] || packet[5]+packet[6]<packet[5]) {
            logline(L"Invalid reported buffer range; stopping.\r\n");return FALSE;
        }
        logline(L"Reported capsule buffer address=0x%lx bytes=%ld (not accessed)\r\n",packet[5],packet[6]);
#ifdef PHOENIX_CAPSULE_PROBE
        if(packet[5]!=0x34cab018 || packet[6]!=31457280) return FALSE;
#endif
    } else {
        logline(L"Hash option fields: %d, %d\r\n",read32((UINT8 *)packet+40),read32((UINT8 *)packet+44));
#ifdef PHOENIX_SIGNATURE_PROBE
        if(packet[5]!=0) {logline(L"Hash options changed; fixed-image signature probe refused.\r\n");return FALSE;}
#endif
    }
    return TRUE;
}

static EFI_STATUS save(EFI_HANDLE image) {
    EFI_GUID loaded=LOADED_IMAGE_PROTOCOL, filesystem=EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID;
    EFI_LOADED_IMAGE *li=NULL;EFI_FILE_IO_INTERFACE *fs=NULL;EFI_FILE_HANDLE root=NULL,file=NULL;
    EFI_STATUS s=uefi_call_wrapper(BS->HandleProtocol,3,image,&loaded,(VOID **)&li);
    if(!EFI_ERROR(s)) s=uefi_call_wrapper(BS->HandleProtocol,3,li->DeviceHandle,&filesystem,(VOID **)&fs);
    if(!EFI_ERROR(s)) s=uefi_call_wrapper(fs->OpenVolume,2,fs,&root);
    if(!EFI_ERROR(s)) s=uefi_call_wrapper(root->Open,5,root,&file,L"\\EFI\\CognacPhoenix\\info.txt",
        EFI_FILE_MODE_READ|EFI_FILE_MODE_WRITE|EFI_FILE_MODE_CREATE,0);
    if(!EFI_ERROR(s)) {
        // Remove old content so a shorter failure report cannot leave stale success text.
        s=uefi_call_wrapper(file->Delete,1,file);file=NULL;
        if(!EFI_ERROR(s)) s=uefi_call_wrapper(root->Open,5,root,&file,L"\\EFI\\CognacPhoenix\\info.txt",
            EFI_FILE_MODE_READ|EFI_FILE_MODE_WRITE|EFI_FILE_MODE_CREATE,0);
        if(!EFI_ERROR(s)) {
            UINTN n=loglen;s=uefi_call_wrapper(file->Write,3,file,&n,logbuf);
            if(!EFI_ERROR(s) && n!=loglen) s=EFI_DEVICE_ERROR;
            uefi_call_wrapper(file->Flush,1,file);uefi_call_wrapper(file->Close,1,file);
        }
    }
    if(root) uefi_call_wrapper(root->Close,1,root);
    return s;
}
#ifdef PHOENIX_SIGNATURE_PROBE
#include "signature-query.h"
#endif
#ifdef PHOENIX_CAPSULE_PROBE
#ifdef PHOENIX_FLASH_ONESHOT
#include "flash-stage.h"
#endif
#include "capsule-query.h"
#endif
EFI_STATUS efi_main(EFI_HANDLE image,EFI_SYSTEM_TABLE *st) {
    InitializeLib(image,st);
#ifdef PHOENIX_FLASH_ONESHOT
    logline(L"Cognac authorized Phoenix update v10. Press I to validate and, only if accepted, stage and warm reset for flash.\r\nKeep AC power connected; do not interrupt the firmware update.\r\n");
#else
#ifdef PHOENIX_CAPSULE_PROBE
    logline(L"Cognac full capsule-validation probe v8. No flash or update variables.\r\n");
#endif
#ifdef PHOENIX_SIGNATURE_PROBE
    logline(L"Cognac Phoenix signature-validation probe v6\r\nPressing I also queries trust for the verified FXCN49WW signature.\r\n");
#endif
    logline(L"Cognac Phoenix GET_INFO probe v5 -- experimental\r\nNo BIOS flash, no variable changes, no automatic reboot.\r\n");
#endif
    UINTN key;UINT32 descriptor_version;
    EFI_STATUS s=uefi_call_wrapper(BS->GetMemoryMap,5,&map_size,NULL,&key,&descriptor_size,&descriptor_version);
    if(s!=EFI_BUFFER_TOO_SMALL || descriptor_size<sizeof(EFI_MEMORY_DESCRIPTOR)) return EFI_UNSUPPORTED;
    map_size+=descriptor_size*16;map=AllocatePool(map_size);
    if(!map) return EFI_OUT_OF_RESOURCES;
    s=uefi_call_wrapper(BS->GetMemoryMap,5,&map_size,map,&key,&descriptor_size,&descriptor_version);
    logline(L"GetMemoryMap status=%r bytes=%ld stride=%ld version=%d\r\n",s,(UINT64)map_size,(UINT64)descriptor_size,descriptor_version);
    if(EFI_ERROR(s) || !platform(st)) {
        logline(L"Platform/table/memory-range validation failed. No SMI issued.\r\n");
    } else {
        logline(L"Exact Lenovo FXCN28WW table snapshot matches.\r\nSMI port=0xb2 command=0xe9 shared RAM=0x44cd4000\r\n");
#ifdef PHOENIX_FLASH_ONESHOT
        logline(L"Press I for authorized validation and conditional update; any other key skips.\r\n");
#else
        logline(L"Press I to query service info, capsule buffer and hash options; any other key skips.\r\n");
#endif
        UINTN event;EFI_INPUT_KEY input;
        s=uefi_call_wrapper(BS->WaitForEvent,3,1,&st->ConIn->WaitForKey,&event);
        if(!EFI_ERROR(s)) s=uefi_call_wrapper(st->ConIn->ReadKeyStroke,2,st->ConIn,&input);
        if(!EFI_ERROR(s) && (input.UnicodeChar==L'i' || input.UnicodeChar==L'I')) {
            UINT64 request[64]={0x10000,0xff,104};
            const UINT8 guid[16]={0xfc,0x44,0xde,0xb1,0x46,0x79,0x82,0x49,0x9b,0x4b,0x2f,0x8c,0xa4,0x5e,0xa7,0x92};
            CopyMem((UINT8 *)request+24,(VOID *)guid,16);
            volatile UINT8 *shared=(VOID *)(UINTN)0x44cd4000;
            for(UINTN i=0;i<104;i++) shared[i]=((UINT8 *)request)[i];
            __asm__ __volatile__("mfence; outb %0,%1; mfence"::"a"((UINT8)0xe9),"d"((UINT16)0xb2):"memory");
            for(UINTN i=0;i<104;i++) ((UINT8 *)request)[i]=shared[i];
            UINTN captured=104;
            if(request[2]>104 && request[2]<=sizeof(request)) {
                captured=(UINTN)request[2];
                for(UINTN i=104;i<captured;i++) ((UINT8 *)request)[i]=shared[i];
            }
            logline(L"Raw GET_INFO response, %ld bytes:\r\n",(UINT64)captured);
            for(UINTN i=0;i<captured;i++) logline(L"%02x%s",((UINT8 *)request)[i],((i+1)%16)?L" ":L"\r\n");
            logline(L"\r\n");
            const UINT8 zero_guid[16]={0};
            if(request[0]!=0x10000 || (request[2]!=104 && request[2]!=112)
               || request[5]!=request[2]-40
               || (CompareMem((UINT8 *)request+24,guid,16) && CompareMem((UINT8 *)request+24,zero_guid,16)))
                logline(L"Response identity/size mismatch; response is unverified.\r\n");
            else {
                logline(L"Firmware GET_INFO status=0x%lx\r\n",request[1]);
                logline(L"Service-info declared size=0x%lx\r\n",request[5]);
                logline(L"Raw service-info bytes (layout still under analysis):\r\n");
                for(UINTN i=40;i<captured;i++) logline(L"%02x%s",((UINT8 *)request)[i],((i-39)%16)?L" ":L"\r\n");
                logline(L"\r\nThis response does not establish capsule validation or flash success.\r\n");
                if(request[1]==0 && !EFI_ERROR(save(image)) && capability_query(TRUE)) {
                    if(!EFI_ERROR(save(image)) && capability_query(FALSE)) {
#ifdef PHOENIX_SIGNATURE_PROBE
                        if(!EFI_ERROR(save(image)) && signature_query()) {
#ifdef PHOENIX_CAPSULE_PROBE
                            if(!EFI_ERROR(save(image))) capsule_query(image);
#endif
                        }
#endif
                    }
                }
            }
        } else logline(L"Query skipped. No SMI issued.\r\n");
    }
    FreePool(map);
    s=save(image);Print(L"\r\nLog save: %r\r\nReturning to boot manager in 15 seconds.\r\n",s);
    uefi_call_wrapper(BS->Stall,1,(UINTN)15000000);
    return EFI_SUCCESS;
}
