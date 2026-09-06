#include "trdp_bridge.h"
#include "vos_utils.h"

#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <process.h>
#else
#include <dlfcn.h>
#endif

#define LINKTYPE_NULL 0
#define LINKTYPE_ETHERNET 1
#define LINKTYPE_RAW 101
#define LINKTYPE_LINUX_SLL 113
#define LINKTYPE_LINUX_SLL2 276
#define CAPTURE_COUNT 2

typedef struct pcap pcap_t;
typedef unsigned int bpf_u_int32;

struct bridge_pcap_pkthdr {
    struct timeval ts;
    bpf_u_int32 caplen;
    bpf_u_int32 len;
};

struct bridge_bpf_insn {
    unsigned short code;
    unsigned char jt;
    unsigned char jf;
    bpf_u_int32 k;
};

struct bridge_bpf_program {
    unsigned int bf_len;
    struct bridge_bpf_insn *bf_insns;
};

struct bridge_pcap_if {
    struct bridge_pcap_if *next;
    char *name;
    char *description;
    void *addresses;
    bpf_u_int32 flags;
};

typedef pcap_t *(*fn_pcap_open_live)(const char *, int, int, int, char *);
typedef int (*fn_pcap_next_ex)(pcap_t *, struct bridge_pcap_pkthdr **, const unsigned char **);
typedef void (*fn_pcap_close)(pcap_t *);
typedef int (*fn_pcap_compile)(pcap_t *, struct bridge_bpf_program *, const char *, int, bpf_u_int32);
typedef int (*fn_pcap_setfilter)(pcap_t *, struct bridge_bpf_program *);
typedef void (*fn_pcap_freecode)(struct bridge_bpf_program *);
typedef int (*fn_pcap_datalink)(pcap_t *);
typedef void (*fn_pcap_breakloop)(pcap_t *);
typedef const char *(*fn_pcap_geterr)(pcap_t *);
typedef int (*fn_pcap_findalldevs)(struct bridge_pcap_if **, char *);
typedef void (*fn_pcap_freealldevs)(struct bridge_pcap_if *);

typedef struct {
    pcap_t *pcap;
    bridge_thread_t thread;
    char interface_name[512];
    char label;
    int linktype;
    int thread_active;
} capture_context_t;

static fn_pcap_open_live dyn_open_live;
static fn_pcap_next_ex dyn_next_ex;
static fn_pcap_close dyn_close;
static fn_pcap_compile dyn_compile;
static fn_pcap_setfilter dyn_setfilter;
static fn_pcap_freecode dyn_freecode;
static fn_pcap_datalink dyn_datalink;
static fn_pcap_breakloop dyn_breakloop;
static fn_pcap_geterr dyn_geterr;
static fn_pcap_findalldevs dyn_findalldevs;
static fn_pcap_freealldevs dyn_freealldevs;
static void *g_pcap_library;
static capture_context_t g_capture[CAPTURE_COUNT];

static void *dynamic_symbol(const char *name) {
#ifdef _WIN32
    return (void *)GetProcAddress((HMODULE)g_pcap_library, name);
#else
    return dlsym(g_pcap_library, name);
#endif
}

static void unload_pcap(void) {
    if (g_pcap_library == NULL) {
        return;
    }
#ifdef _WIN32
    (void)FreeLibrary((HMODULE)g_pcap_library);
#else
    (void)dlclose(g_pcap_library);
#endif
    g_pcap_library = NULL;
    dyn_open_live = NULL;
    dyn_next_ex = NULL;
    dyn_close = NULL;
    dyn_compile = NULL;
    dyn_setfilter = NULL;
    dyn_freecode = NULL;
    dyn_datalink = NULL;
    dyn_breakloop = NULL;
    dyn_geterr = NULL;
    dyn_findalldevs = NULL;
    dyn_freealldevs = NULL;
}

static int load_pcap(void) {
    if (g_pcap_library != NULL) {
        return 1;
    }
#ifdef _WIN32
    {
        char system_directory[MAX_PATH];
        char path[MAX_PATH + 32];
        UINT length = GetSystemDirectoryA(system_directory, MAX_PATH);
        if (length > 0u && length < MAX_PATH) {
            (void)snprintf(path, sizeof(path), "%s\\Npcap\\wpcap.dll", system_directory);
            g_pcap_library = (void *)LoadLibraryExA(
                path,
                NULL,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            );
        }
        if (g_pcap_library == NULL && length > 0u && length < MAX_PATH) {
            (void)snprintf(path, sizeof(path), "%s\\wpcap.dll", system_directory);
            g_pcap_library = (void *)LoadLibraryExA(
                path,
                NULL,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32
            );
        }
    }
#else
#ifdef __APPLE__
    g_pcap_library = dlopen("/usr/lib/libpcap.A.dylib", RTLD_NOW);
    if (g_pcap_library == NULL) {
        g_pcap_library = dlopen("libpcap.dylib", RTLD_NOW);
    }
#else
    g_pcap_library = dlopen("libpcap.so.1", RTLD_NOW);
    if (g_pcap_library == NULL) {
        g_pcap_library = dlopen("libpcap.so.0.8", RTLD_NOW);
    }
    if (g_pcap_library == NULL) {
        g_pcap_library = dlopen("libpcap.so", RTLD_NOW);
    }
#endif
#endif
    if (g_pcap_library == NULL) {
        return 0;
    }

    dyn_open_live = (fn_pcap_open_live)dynamic_symbol("pcap_open_live");
    dyn_next_ex = (fn_pcap_next_ex)dynamic_symbol("pcap_next_ex");
    dyn_close = (fn_pcap_close)dynamic_symbol("pcap_close");
    dyn_compile = (fn_pcap_compile)dynamic_symbol("pcap_compile");
    dyn_setfilter = (fn_pcap_setfilter)dynamic_symbol("pcap_setfilter");
    dyn_freecode = (fn_pcap_freecode)dynamic_symbol("pcap_freecode");
    dyn_datalink = (fn_pcap_datalink)dynamic_symbol("pcap_datalink");
    dyn_breakloop = (fn_pcap_breakloop)dynamic_symbol("pcap_breakloop");
    dyn_geterr = (fn_pcap_geterr)dynamic_symbol("pcap_geterr");
    dyn_findalldevs = (fn_pcap_findalldevs)dynamic_symbol("pcap_findalldevs");
    dyn_freealldevs = (fn_pcap_freealldevs)dynamic_symbol("pcap_freealldevs");
    if (dyn_open_live == NULL || dyn_next_ex == NULL || dyn_close == NULL
        || dyn_compile == NULL || dyn_setfilter == NULL || dyn_freecode == NULL
        || dyn_datalink == NULL) {
        unload_pcap();
        return 0;
    }
    return 1;
}

void capture_list(void) {
    char error[256] = {0};
    struct bridge_pcap_if *devices = NULL;
    struct bridge_pcap_if *device;
    int first = 1;

    if (!load_pcap()) {
        bridge_emit_error("pcap-compatible capture runtime is unavailable");
        return;
    }
    if (dyn_findalldevs == NULL || dyn_freealldevs == NULL) {
        bridge_emit_error("pcap interface enumeration is unavailable");
        return;
    }
    if (dyn_findalldevs(&devices, error) != 0) {
        bridge_emit_error(*error != '\0' ? error : "pcap_findalldevs failed");
        return;
    }

    bridge_output_lock();
    fputs("{\"event\":\"capture_interfaces\",\"interfaces\":[", stdout);
    for (device = devices; device != NULL; device = device->next) {
        if (device->name == NULL || *device->name == '\0') {
            continue;
        }
        if (!first) {
            fputc(',', stdout);
        }
        first = 0;
        fputs("{\"name\":\"", stdout);
        bridge_json_escape(stdout, device->name);
        fputs("\",\"description\":\"", stdout);
        bridge_json_escape(stdout, device->description != NULL ? device->description : "");
        fputs("\"}", stdout);
    }
    fputs("]}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();

    dyn_freealldevs(devices);
}

static void emit_capture_frame(
    capture_context_t *context,
    const struct bridge_pcap_pkthdr *header,
    const unsigned char *frame
) {
    bridge_output_lock();
    fprintf(
        stdout,
        "{\"event\":\"capture_frame\",\"link\":\"%c\",\"link_type\":%d,"
        "\"timestamp_us\":%llu,\"raw_frame_hex\":\"",
        context->label,
        context->linktype,
        (unsigned long long)header->ts.tv_sec * 1000000ULL
            + (unsigned long long)header->ts.tv_usec
    );
    bridge_print_hex(stdout, frame, header->caplen);
    fputs("\"}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

/*
 * The native capture layer deliberately stays transport-agnostic: it owns
 * libpcap/Npcap access only. Rust receives the raw frame and performs the
 * canonical UDP/TCP/TRDP decode and TCP stream reassembly for both live and
 * offline capture paths.
 */
static void process_frame(
    capture_context_t *context,
    const struct bridge_pcap_pkthdr *header,
    const unsigned char *frame
) {
    if (context == NULL || header == NULL || frame == NULL) {
        return;
    }
    emit_capture_frame(context, header, frame);
}

#ifdef _WIN32
static unsigned __stdcall capture_loop(void *context_ptr)
#else
static void *capture_loop(void *context_ptr)
#endif
{
    capture_context_t *context = (capture_context_t *)context_ptr;
    while (context->pcap != NULL) {
        struct bridge_pcap_pkthdr *header = NULL;
        const unsigned char *data = NULL;
        int result = dyn_next_ex(context->pcap, &header, &data);
        if (result == 1) {
            process_frame(context, header, data);
        } else if (result < 0) {
            break;
        }
    }
#ifdef _WIN32
    return 0u;
#else
    return NULL;
#endif
}

static void stop_context(capture_context_t *context) {
    if (context == NULL) {
        return;
    }
    if (context->pcap != NULL && dyn_breakloop != NULL) {
        dyn_breakloop(context->pcap);
    }
    if (context->thread_active) {
        bridge_thread_join(context->thread);
        context->thread_active = 0;
    }
    if (context->pcap != NULL && dyn_close != NULL) {
        dyn_close(context->pcap);
        context->pcap = NULL;
    }
    context->interface_name[0] = '\0';
}

static int start_context(
    capture_context_t *context,
    char label,
    const char *interface_name,
    const char *filter,
    char *error_message,
    size_t error_capacity
) {
    char pcap_error[256] = {0};
    struct bridge_bpf_program program;

    memset(context, 0, sizeof(*context));
    context->label = label;
    (void)snprintf(context->interface_name, sizeof(context->interface_name), "%s", interface_name);
    context->pcap = dyn_open_live(interface_name, 65535, 1, 100, pcap_error);
    if (context->pcap == NULL) {
        (void)snprintf(
            error_message,
            error_capacity,
            "capture %c open failed: %s",
            label,
            *pcap_error != '\0' ? pcap_error : "pcap_open_live failed"
        );
        return 0;
    }
    context->linktype = dyn_datalink(context->pcap);
    if (context->linktype != LINKTYPE_ETHERNET
        && context->linktype != LINKTYPE_LINUX_SLL
        && context->linktype != LINKTYPE_LINUX_SLL2
        && context->linktype != LINKTYPE_NULL
        && context->linktype != LINKTYPE_RAW) {
        (void)snprintf(error_message, error_capacity, "capture %c link type %d is not supported", label, context->linktype);
        stop_context(context);
        return 0;
    }
    memset(&program, 0, sizeof(program));
    if (dyn_compile(context->pcap, &program, filter, 1, 0xffffffffu) != 0) {
        const char *error = dyn_geterr != NULL ? dyn_geterr(context->pcap) : NULL;
        (void)snprintf(error_message, error_capacity, "capture %c filter compile failed: %s", label, error != NULL ? error : "unknown error");
        stop_context(context);
        return 0;
    }
    if (dyn_setfilter(context->pcap, &program) != 0) {
        const char *error = dyn_geterr != NULL ? dyn_geterr(context->pcap) : NULL;
        dyn_freecode(&program);
        (void)snprintf(error_message, error_capacity, "capture %c setfilter failed: %s", label, error != NULL ? error : "unknown error");
        stop_context(context);
        return 0;
    }
    dyn_freecode(&program);
#ifdef _WIN32
    {
        uintptr_t thread = _beginthreadex(NULL, 0, capture_loop, context, 0, NULL);
        if (thread == 0u) {
            (void)snprintf(error_message, error_capacity, "capture %c thread failed", label);
            stop_context(context);
            return 0;
        }
        context->thread = (HANDLE)thread;
    }
#else
    if (pthread_create(&context->thread, NULL, capture_loop, context) != 0) {
        (void)snprintf(error_message, error_capacity, "capture %c thread failed", label);
        stop_context(context);
        return 0;
    }
#endif
    context->thread_active = 1;
    return 1;
}

void capture_stop(void) {
    int index;
    for (index = 0; index < CAPTURE_COUNT; ++index) {
        stop_context(&g_capture[index]);
    }
}

void capture_start(const char *line) {
    char interface_a[512] = {0};
    char interface_b[512] = {0};
    char filter[1024] = "udp port 17224 or udp port 17225 or tcp port 17225";
    char error[512] = {0};

    if (!bridge_json_string(line, "interface", interface_a, sizeof(interface_a), NULL)
        || *interface_a == '\0') {
        bridge_emit_error("live capture requires interface name");
        return;
    }
    (void)bridge_json_string(line, "interface_b", interface_b, sizeof(interface_b), "");
    (void)bridge_json_string(line, "filter", filter, sizeof(filter), filter);
    if (!load_pcap()) {
        bridge_emit_error("pcap-compatible capture runtime not found. Install a system capture driver/runtime that provides the pcap API.");
        return;
    }
    capture_stop();
    if (!start_context(&g_capture[0], 'A', interface_a, filter, error, sizeof(error))) {
        bridge_emit_error(error);
        return;
    }
    if (*interface_b != '\0'
        && !start_context(&g_capture[1], 'B', interface_b, filter, error, sizeof(error))) {
        capture_stop();
        bridge_emit_error(error);
        return;
    }
    bridge_emit_ack("capture_start", NULL);
}

void capture_shutdown(void) {
    capture_stop();
    unload_pcap();
}
