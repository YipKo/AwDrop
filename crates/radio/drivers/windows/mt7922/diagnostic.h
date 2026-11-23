/* SPDX-License-Identifier: BSD-3-Clause-Clear */
#pragma once
#include <ntddk.h>
#include <wdf.h>
#include <initguid.h>

/* AwDrop diagnostic interface. No raw register or device DMA controls exist. */
DEFINE_GUID(GUID_DEVINTERFACE_AWDROP_MT7922_DIAGNOSTIC,
    0x49e5285d, 0x962b, 0x4d98, 0xa7, 0xc0, 0x5c, 0x72, 0xe8, 0x60, 0xa8, 0x16);

#define IOCTL_AWDROP_MT7922_GET_INFO CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_READ_DATA)
#define IOCTL_AWDROP_MT7922_DMA_TEST CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_READ_DATA)
#define IOCTL_AWDROP_MT7922_OWN_TEST CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_WRITE_DATA)
#define IOCTL_AWDROP_MT7922_RESET_INFO CTL_CODE(FILE_DEVICE_UNKNOWN, 0x803, METHOD_BUFFERED, FILE_READ_DATA)
#define AWDROP_DIAG_VERSION 1u
#define AWDROP_DIAG_DMA_VERSION 3u

typedef struct AWDROP_DIAG_INFO {
    ULONG size;
    ULONG version;
    ULONG vendor;
    ULONG device;
    ULONG subsystem_vendor;
    ULONG subsystem_device;
    ULONG revision;
    ULONG radio_ready; /* Always zero: this is not a radio driver. */
    ULONG memory_resources;
    ULONG interrupt_resources;
    ULONG bar0_bytes;  /* Zero means BAR0 could not be matched. */
    ULONG bar0_matches;
} AWDROP_DIAG_INFO;

typedef struct AWDROP_DIAG_DMA_INFO {
    ULONG size;
    ULONG version;
    ULONG allocated_bytes;
    ULONG address_bits;
    ULONG alignment;
} AWDROP_DIAG_DMA_INFO;

typedef struct AWDROP_DIAG_OWN_INFO {
    ULONG size;
    ULONG version;
    ULONG result;
    ULONG dma_valid;
    ULONG dma_flags; /* WFDMA TX/RX enable and busy bits, if dma_valid. */
} AWDROP_DIAG_OWN_INFO;

typedef struct AWDROP_DIAG_RESET_INFO {
    ULONG size;
    ULONG version;
    ULONG queried;
    ULONG supported_types; /* Bit 0: function; bit 1: platform. No reset is invoked. */
} AWDROP_DIAG_RESET_INFO;

typedef struct AWDROP_DIAG_DEVICE {
    AWDROP_DIAG_INFO info;
    AWDROP_DIAG_RESET_INFO reset_info;
    BOOLEAN prepared;
    PVOID bar0_mapping;
    SIZE_T bar0_mapping_length;
    volatile LONG surprise_removed;
    BOOLEAN ownership_unsafe;
} AWDROP_DIAG_DEVICE;
WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(AWDROP_DIAG_DEVICE, AwGetDevice)

DRIVER_INITIALIZE DriverEntry;
EVT_WDF_DRIVER_DEVICE_ADD AwDeviceAdd;
EVT_WDF_DEVICE_PREPARE_HARDWARE AwPrepare;
EVT_WDF_DEVICE_RELEASE_HARDWARE AwRelease;
EVT_WDF_DEVICE_SURPRISE_REMOVAL AwSurpriseRemoval;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL AwControl;
