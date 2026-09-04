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
#define TCP_FLOW_COUNT 32
#define TCP_BUFFER_CAP 131072u

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

typedef pcap_t *(*fn_pcap_open_live)(const char *, int, int, int, char *);
typedef int (*fn_pcap_next_ex)(pcap_t *, struct bridge_pcap_pkthdr **, const unsigned char **);
typedef void (*fn_pcap_close)(pcap_t *);
typedef int (*fn_pcap_compile)(pcap_t *, struct bridge_bpf_program *, const char *, int, bpf_u_int32);
typedef int (*fn_pcap_setfilter)(pcap_t *, struct bridge_bpf_program *);
typedef void (*fn_pcap_freecode)(struct bridge_bpf_program *);
typedef int (*fn_pcap_datalink)(pcap_t *);
typedef void (*fn_pcap_breakloop)(pcap_t *);
typedef const char *(*fn_pcap_geterr)(pcap_t *);

typedef struct {
    uint32_t source_ip;
    uint32_t destination_ip;
    uint16_t source_port;
    uint16_t destination_port;
    uint32_t expected_sequence;
    unsigned char *buffer;
    size_t length;
    int initialized;
    int active;
} tcp_flow_t;

typedef struct {
    pcap_t *pcap;
    bridge_thread_t thread;
    tcp_flow_t flows[TCP_FLOW_COUNT];
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
static void *g_pcap_library;
static capture_context_t g_capture[CAPTURE_COUNT];

static uint16_t read_be16(const unsigned char *data) {
    return (uint16_t)(((uint16_t)data[0] << 8) | (uint16_t)data[1]);
}

static uint32_t read_be32(const unsigned char *data) {
    return ((uint32_t)data[0] << 24)
        | ((uint32_t)data[1] << 16)
        | ((uint32_t)data[2] << 8)
        | (uint32_t)data[3];
}

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
    if (dyn_open_live == NULL || dyn_next_ex == NULL || dyn_close == NULL
        || dyn_compile == NULL || dyn_setfilter == NULL || dyn_freecode == NULL
        || dyn_datalink == NULL) {
        unload_pcap();
        return 0;
    }
    return 1;
}

static int network_offset(
    const unsigned char *frame,
    size_t length,
    int linktype,
    size_t *offset
) {
    if (linktype == LINKTYPE_ETHERNET) {
        uint16_t ether_type;
        size_t cursor = 14u;
        if (length < cursor) {
            return 0;
        }
        ether_type = read_be16(frame + 12u);
        while (ether_type == 0x8100u || ether_type == 0x88a8u || ether_type == 0x9100u) {
            if (length < cursor + 4u) {
                return 0;
            }
            ether_type = read_be16(frame + cursor + 2u);
            cursor += 4u;
        }
        if (ether_type != 0x0800u) {
            return 0;
        }
        *offset = cursor;
        return 1;
    }
    if (linktype == LINKTYPE_LINUX_SLL) {
        if (length < 16u || read_be16(frame + 14u) != 0x0800u) {
            return 0;
        }
        *offset = 16u;
        return 1;
    }
    if (linktype == LINKTYPE_LINUX_SLL2) {
        if (length < 20u || read_be16(frame) != 0x0800u) {
            return 0;
        }
        *offset = 20u;
        return 1;
    }
    if (linktype == LINKTYPE_NULL) {
        if (length < 4u || (frame[4] >> 4) != 4u) {
            return 0;
        }
        *offset = 4u;
        return 1;
    }
    if (linktype == LINKTYPE_RAW) {
        if (length < 1u || (frame[0] >> 4) != 4u) {
            return 0;
        }
        *offset = 0u;
        return 1;
    }
    return 0;
}

static int valid_md_type(const unsigned char *data, size_t length) {
    if (length < 24u || data[6] != 'M') {
        return 0;
    }
    return data[7] == 'n' || data[7] == 'r' || data[7] == 'p'
        || data[7] == 'q' || data[7] == 'c' || data[7] == 'e';
}

static int valid_pd_type(const unsigned char *data, size_t length) {
    if (length < 24u || data[6] != 'P') {
        return 0;
    }
    return data[7] == 'd' || data[7] == 'p' || data[7] == 'r' || data[7] == 'e';
}

static void emit_trdp(
    capture_context_t *context,
    const struct bridge_pcap_pkthdr *header,
    const unsigned char *raw_frame,
    uint32_t source_ip,
    uint32_t destination_ip,
    uint16_t source_port,
    uint16_t destination_port,
    const char *transport,
    const unsigned char *trdp,
    size_t trdp_length
) {
    size_t header_length;
    size_t data_length;
    size_t available;
    UINT32 stored_fcs;
    UINT32 computed_fcs;
    int crc_valid;
    int protocol_valid;
    char message_type[3];

    if (valid_md_type(trdp, trdp_length)) {
        header_length = 116u;
    } else if (valid_pd_type(trdp, trdp_length)) {
        header_length = 40u;
    } else {
        return;
    }
    if (trdp_length < header_length) {
        return;
    }
    data_length = (size_t)read_be32(trdp + 20u);
    available = trdp_length - header_length;
    if (available > data_length) {
        available = data_length;
    }
    message_type[0] = (char)trdp[6];
    message_type[1] = (char)trdp[7];
    message_type[2] = '\0';
    memcpy(&stored_fcs, trdp + header_length - SIZE_OF_FCS, SIZE_OF_FCS);
    computed_fcs = vos_crc32(INITFCS, trdp, (UINT32)(header_length - SIZE_OF_FCS));
    crc_valid = stored_fcs == MAKE_LE(computed_fcs);
    protocol_valid = (read_be16(trdp + 4u) & 0xff00u) == 0x0100u;

    bridge_output_lock();
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"capture\",\"link\":\"%c\","
        "\"link_type\":%d,\"timestamp_us\":%llu,\"transport\":\"%s\","
        "\"src_port\":%u,\"dest_port\":%u,\"msg_type\":\"%s\","
        "\"com_id\":%u,\"seq_count\":%u,\"protocol_version\":%u,"
        "\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,"
        "\"crc_valid\":%s,\"protocol_valid\":%s,"
        "\"src_ip\":\"%u.%u.%u.%u\",\"dest_ip\":\"%u.%u.%u.%u\","
        "\"data_len\":%u,\"payload_hex\":\"",
        context->label,
        context->linktype,
        (unsigned long long)header->ts.tv_sec * 1000000ULL + (unsigned long long)header->ts.tv_usec,
        transport,
        (unsigned int)source_port,
        (unsigned int)destination_port,
        message_type,
        (unsigned int)read_be32(trdp + 8u),
        (unsigned int)read_be32(trdp),
        (unsigned int)read_be16(trdp + 4u),
        (unsigned int)read_be32(trdp + 12u),
        (unsigned int)read_be32(trdp + 16u),
        crc_valid ? "true" : "false",
        protocol_valid ? "true" : "false",
        (unsigned int)((source_ip >> 24) & 0xffu),
        (unsigned int)((source_ip >> 16) & 0xffu),
        (unsigned int)((source_ip >> 8) & 0xffu),
        (unsigned int)(source_ip & 0xffu),
        (unsigned int)((destination_ip >> 24) & 0xffu),
        (unsigned int)((destination_ip >> 16) & 0xffu),
        (unsigned int)((destination_ip >> 8) & 0xffu),
        (unsigned int)(destination_ip & 0xffu),
        (unsigned int)data_length
    );
    bridge_print_hex(stdout, trdp + header_length, (UINT32)available);
    fputs("\",\"raw_frame_hex\":\"", stdout);
    bridge_print_hex(stdout, raw_frame, header->caplen);
    fputs("\"", stdout);
    if (valid_md_type(trdp, trdp_length) && trdp_length >= 116u) {
        static const char digits[] = "0123456789abcdef";
        char source_uri[33] = {0};
        char destination_uri[33] = {0};
        int32_t raw_reply_status = (int32_t)read_be32(trdp + 24u);
        int32_t reply_status = raw_reply_status >= 0 ? 0 : raw_reply_status;
        uint16_t user_status = raw_reply_status >= 0 ? (uint16_t)raw_reply_status : 0u;
        size_t index;

        memcpy(source_uri, trdp + 48u, 32u);
        memcpy(destination_uri, trdp + 80u, 32u);
        fprintf(
            stdout,
            ",\"reply_status\":%d,\"user_status\":%u,\"reply_timeout_us\":%u,"
            "\"src_uri\":\"",
            (int)reply_status,
            (unsigned int)user_status,
            (unsigned int)read_be32(trdp + 44u)
        );
        bridge_json_escape(stdout, source_uri);
        fputs("\",\"dest_uri\":\"", stdout);
        bridge_json_escape(stdout, destination_uri);
        fputs("\",\"md_session_id\":\"", stdout);
        for (index = 0u; index < 16u; ++index) {
            unsigned char value = trdp[28u + index];
            fputc(digits[(value >> 4) & 0x0fu], stdout);
            fputc(digits[value & 0x0fu], stdout);
            if (index == 3u || index == 5u || index == 7u || index == 9u) {
                fputc('-', stdout);
            }
        }
        fputc('"', stdout);
    }
    fputs("}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

static void reset_flow(tcp_flow_t *flow) {
    if (flow == NULL) {
        return;
    }
    free(flow->buffer);
    memset(flow, 0, sizeof(*flow));
}

static tcp_flow_t *flow_for(
    capture_context_t *context,
    uint32_t source_ip,
    uint32_t destination_ip,
    uint16_t source_port,
    uint16_t destination_port
) {
    int index;
    tcp_flow_t *free_slot = NULL;
    for (index = 0; index < TCP_FLOW_COUNT; ++index) {
        tcp_flow_t *flow = &context->flows[index];
        if (flow->active
            && flow->source_ip == source_ip
            && flow->destination_ip == destination_ip
            && flow->source_port == source_port
            && flow->destination_port == destination_port) {
            return flow;
        }
        if (!flow->active && free_slot == NULL) {
            free_slot = flow;
        }
    }
    if (free_slot == NULL) {
        free_slot = &context->flows[0];
        reset_flow(free_slot);
    }
    free_slot->buffer = (unsigned char *)malloc(TCP_BUFFER_CAP);
    if (free_slot->buffer == NULL) {
        return NULL;
    }
    free_slot->source_ip = source_ip;
    free_slot->destination_ip = destination_ip;
    free_slot->source_port = source_port;
    free_slot->destination_port = destination_port;
    free_slot->active = 1;
    return free_slot;
}

static void process_tcp_payload(
    capture_context_t *context,
    const struct bridge_pcap_pkthdr *header,
    const unsigned char *frame,
    uint32_t source_ip,
    uint32_t destination_ip,
    uint16_t source_port,
    uint16_t destination_port,
    uint32_t sequence,
    unsigned char flags,
    const unsigned char *payload,
    size_t payload_length
) {
    tcp_flow_t *flow;
    uint32_t payload_sequence = sequence + ((flags & 0x02u) != 0u ? 1u : 0u);
    size_t trim = 0u;

    flow = flow_for(context, source_ip, destination_ip, source_port, destination_port);
    if (flow == NULL) {
        return;
    }
    if ((flags & 0x04u) != 0u || (flags & 0x02u) != 0u) {
        flow->length = 0u;
        flow->expected_sequence = payload_sequence;
        flow->initialized = 1;
    }
    if (!flow->initialized) {
        flow->expected_sequence = payload_sequence;
        flow->initialized = 1;
    }
    if (payload_length > 0u) {
        int32_t delta = (int32_t)(payload_sequence - flow->expected_sequence);
        if (delta > 0) {
            flow->length = 0u;
            flow->expected_sequence = payload_sequence;
        } else if (delta < 0) {
            uint32_t overlap = flow->expected_sequence - payload_sequence;
            if ((size_t)overlap >= payload_length) {
                payload_length = 0u;
            } else {
                trim = (size_t)overlap;
            }
        }
        if (payload_length > trim) {
            size_t append = payload_length - trim;
            if (flow->length + append > TCP_BUFFER_CAP) {
                flow->length = 0u;
                if (append > TCP_BUFFER_CAP) {
                    append = TCP_BUFFER_CAP;
                    trim = payload_length - append;
                }
            }
            memcpy(flow->buffer + flow->length, payload + trim, append);
            flow->length += append;
            flow->expected_sequence += (uint32_t)append;
        }
    }

    while (flow->length >= 24u) {
        size_t start = 0u;
        size_t data_length;
        size_t total;
        while (start + 24u <= flow->length && !valid_md_type(flow->buffer + start, flow->length - start)) {
            ++start;
        }
        if (start > 0u) {
            memmove(flow->buffer, flow->buffer + start, flow->length - start);
            flow->length -= start;
        }
        if (flow->length < 116u || !valid_md_type(flow->buffer, flow->length)) {
            break;
        }
        data_length = (size_t)read_be32(flow->buffer + 20u);
        if (data_length > BRIDGE_MAX_PAYLOAD) {
            flow->length = 0u;
            break;
        }
        total = 116u + data_length;
        if (flow->length < total) {
            break;
        }
        emit_trdp(
            context,
            header,
            frame,
            source_ip,
            destination_ip,
            source_port,
            destination_port,
            "tcp",
            flow->buffer,
            total
        );
        memmove(flow->buffer, flow->buffer + total, flow->length - total);
        flow->length -= total;
    }
    if ((flags & 0x05u) != 0u) {
        reset_flow(flow);
    }
}

static void process_frame(
    capture_context_t *context,
    const struct bridge_pcap_pkthdr *header,
    const unsigned char *frame
) {
    size_t ip_offset;
    size_t ip_header_length;
    size_t transport_offset;
    uint16_t fragment;
    uint32_t source_ip;
    uint32_t destination_ip;
    unsigned char protocol;

    if (header == NULL || frame == NULL
        || !network_offset(frame, (size_t)header->caplen, context->linktype, &ip_offset)
        || (size_t)header->caplen < ip_offset + 20u
        || (frame[ip_offset] >> 4) != 4u) {
        return;
    }
    ip_header_length = (size_t)(frame[ip_offset] & 0x0fu) * 4u;
    if (ip_header_length < 20u || (size_t)header->caplen < ip_offset + ip_header_length) {
        return;
    }
    fragment = read_be16(frame + ip_offset + 6u);
    if ((fragment & 0x3fffu) != 0u) {
        return;
    }
    source_ip = read_be32(frame + ip_offset + 12u);
    destination_ip = read_be32(frame + ip_offset + 16u);
    protocol = frame[ip_offset + 9u];
    transport_offset = ip_offset + ip_header_length;

    if (protocol == 17u) {
        uint16_t source_port;
        uint16_t destination_port;
        size_t trdp_offset;
        size_t udp_length;
        if ((size_t)header->caplen < transport_offset + 8u) {
            return;
        }
        source_port = read_be16(frame + transport_offset);
        destination_port = read_be16(frame + transport_offset + 2u);
        udp_length = (size_t)read_be16(frame + transport_offset + 4u);
        if (udp_length < 8u) {
            return;
        }
        trdp_offset = transport_offset + 8u;
        if ((size_t)header->caplen <= trdp_offset) {
            return;
        }
        emit_trdp(
            context,
            header,
            frame,
            source_ip,
            destination_ip,
            source_port,
            destination_port,
            "udp",
            frame + trdp_offset,
            (size_t)header->caplen - trdp_offset
        );
    } else if (protocol == 6u) {
        size_t tcp_header_length;
        size_t payload_offset;
        uint16_t source_port;
        uint16_t destination_port;
        uint32_t sequence;
        unsigned char flags;
        if ((size_t)header->caplen < transport_offset + 20u) {
            return;
        }
        tcp_header_length = (size_t)(frame[transport_offset + 12u] >> 4) * 4u;
        if (tcp_header_length < 20u || (size_t)header->caplen < transport_offset + tcp_header_length) {
            return;
        }
        source_port = read_be16(frame + transport_offset);
        destination_port = read_be16(frame + transport_offset + 2u);
        sequence = read_be32(frame + transport_offset + 4u);
        flags = frame[transport_offset + 13u];
        payload_offset = transport_offset + tcp_header_length;
        process_tcp_payload(
            context,
            header,
            frame,
            source_ip,
            destination_ip,
            source_port,
            destination_port,
            sequence,
            flags,
            frame + payload_offset,
            (size_t)header->caplen - payload_offset
        );
    }
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
    int index;
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
    for (index = 0; index < TCP_FLOW_COUNT; ++index) {
        reset_flow(&context->flows[index]);
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
        bridge_emit_error("libpcap/Npcap not found. Windows users must install Npcap separately.");
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
