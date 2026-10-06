#define _GNU_SOURCE
#define PHOENIX_FLASH_TEST
#include <efi.h>
#include <efilib.h>
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
static UINT64 read64(const VOID *p) {UINT64 v;memcpy(&v,p,8);return v;}
static VOID logline(CHAR16 *format,...) {(void)format;}
static EFI_STATUS save(EFI_HANDLE image) {(void)image;return EFI_SUCCESS;}
#include "../flash-stage.h"

static UINT8 values[2][32];static UINTN sizes[2];
static unsigned resets,enables,disables,variable_writes,polls;
static int failure_mode;static BOOLEAN enabled;
static EFI_PHYSICAL_ADDRESS test_page;
static int variable_id(CHAR16 *name) {return name[7]==L'U'?0:1;}
static EFI_STATUS EFIAPI allocate(EFI_ALLOCATE_TYPE type,EFI_MEMORY_TYPE memory,UINTN count,EFI_PHYSICAL_ADDRESS *page) {
    assert(type==AllocateMaxAddress && memory==EfiRuntimeServicesData && count==1);
    VOID *p=mmap(NULL,4096,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS|MAP_32BIT,-1,0);
    assert(p!=MAP_FAILED);test_page=(UINTN)p;assert(test_page<=0xfffff000);
    *page=test_page;return EFI_SUCCESS;
}
static EFI_STATUS EFIAPI release(EFI_PHYSICAL_ADDRESS page,UINTN count) {assert(page==test_page && count==1);munmap((VOID *)(UINTN)page,4096);return EFI_SUCCESS;}
static EFI_STATUS EFIAPI stall(UINTN us) {assert(us==5000);return EFI_SUCCESS;}
static EFI_STATUS EFIAPI get_variable(CHAR16 *name,EFI_GUID *guid,UINT32 *attributes,UINTN *n,VOID *data) {
    int id=variable_id(name);assert(!memcmp(guid,id?&capsule_info:&capsule_vendor,16));
    if(!sizes[id]) return EFI_NOT_FOUND;
    if(*n<sizes[id]) {*n=sizes[id];return EFI_BUFFER_TOO_SMALL;}
    *n=sizes[id];*attributes=7;memcpy(data,values[id],sizes[id]);return EFI_SUCCESS;
}
static EFI_STATUS EFIAPI reset(EFI_RESET_TYPE type,EFI_STATUS status,UINTN size,CHAR16 *data) {
    assert(type==EfiResetWarm && status==EFI_SUCCESS && size==0 && data==NULL);resets++;return EFI_SUCCESS;
}
static VOID flash_exchange(UINT8 *packet,UINTN size) {
    UINT64 command=read64(packet);
    if(command==5) {
        assert(size==113 && read64(packet+8)==1 && read64(packet+16)==0xfffffff0);
        assert(read64(packet+48)==test_page && read64(packet+72)==test_page+0x400 && read64(packet+88)==test_page+0x800);
        assert(!memcmp(packet+96,flash_identity,16));
        UINT64 status=failure_mode==1?3:0;memcpy(packet+80,&status,8);
    } else if(command==9) {
        assert(size==113 && !memcmp(packet+96,flash_identity,16));
        UINT64 status=0;
        if(packet[112]) {assert(!enabled);enables++;enabled=TRUE;}
        else {disables++;if(failure_mode==3) status=3;else enabled=FALSE;}
        memcpy(packet+80,&status,8);
    } else if(command==0x10600) {
        assert(enabled && read64(packet+16)==80 && !memcmp(packet+24,flash_identity,16));
        CHAR16 name[40]={0};memcpy(name,packet+80,size-80);int id=variable_id(name);
        assert(!memcmp(packet+40,id?&capsule_info:&capsule_vendor,16));
        UINTN n=read64(packet+64);UINT64 status=0;
        assert(n<=32 && read64(packet+56)==(n?7:0));
        assert(size==80+(StrLen(name)+1)*2);
        variable_writes++;
        if((failure_mode==2 || failure_mode==4) && id==1 && n) status=7;
        else if(failure_mode==4 && n==0) status=7;
        else {sizes[id]=n;if(n) memcpy(values[id],(VOID *)(UINTN)read64(packet+72),n);}
        memcpy(packet+8,&status,8);
    } else if(command==0x13300) {
        assert(size==40);polls++;UINT64 status=polls%2?6:0;memcpy(packet+8,&status,8);
    } else assert(0);
}
int main(void) {
    static EFI_BOOT_SERVICES boot;static EFI_RUNTIME_SERVICES runtime;
    BS=&boot;RT=&runtime;boot.AllocatePages=allocate;boot.FreePages=release;boot.Stall=stall;
    runtime.GetVariable=get_variable;runtime.ResetSystem=reset;
    UINT8 capsule[16]={0xd3,0xaf,0x0b,0xe2};
    for(failure_mode=0;failure_mode<=4;failure_mode++) {
        memset(values,0,sizeof(values));memset(sizes,0,sizeof(sizes));
        resets=enables=disables=variable_writes=polls=0;enabled=FALSE;
        BOOLEAN retain=flash_stage(NULL,capsule);
        if(failure_mode==0) {assert(retain && resets==1 && sizes[0]==8 && sizes[1]==16 && !enabled);}
        if(failure_mode==1) {assert(!retain && resets==0 && enables==0 && variable_writes==0);}
        if(failure_mode==2) {assert(!retain && resets==0 && sizes[0]==0 && sizes[1]==0 && !enabled);}
        if(failure_mode==3) {assert(retain && resets==0 && disables==1);}
        if(failure_mode==4) {assert(retain && resets==0 && sizes[0]==8 && !enabled);}
    }
    puts("Vendor handoff mocks: success, channel rejection, cleanup, disable failure and uncertain cleanup passed");
    return 0;
}
