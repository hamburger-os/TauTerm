/*
 * TauTerm TRDP native bridge
 *
 * TauTerm code in this file is MIT OR Apache-2.0. It links against TCNOpen TRDP
 * (MPL-2.0) as a separate native component. TCNOpen source files retain their own
 * MPL-2.0 notices and are not copied into this file.
 *
 * Protocol: newline-delimited JSON on stdin/stdout.  The parser intentionally
 * accepts only the compact objects emitted by TauTerm, so no general JSON library
 * is required in the native helper.
 */

#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "trdp_if_light.h"
#include "vos_sock.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <process.h>
#include <winsock2.h>
typedef HANDLE bridge_thread_t;
typedef CRITICAL_SECTION bridge_mutex_t;
#define SLEEP_MS(ms) Sleep((DWORD)(ms))
#else
#include <dlfcn.h>
#include <pthread.h>
#include <sys/time.h>
#include <unistd.h>
typedef pthread_t bridge_thread_t;
typedef pthread_mutex_t bridge_mutex_t;
#define SLEEP_MS(ms) usleep((useconds_t)((ms) * 1000u))
#endif

#define MAX_OBJECTS 256
#define MAX_PAYLOAD 65536
#define MAX_LINE 131072
#define TRDP_PD_PORT_TAU 17224u
#define TRDP_MD_PORT_TAU 17225u

typedef enum {
    OBJ_NONE = 0,
    OBJ_PD_PUBLISHER,
    OBJ_PD_SUBSCRIBER,
    OBJ_PD_REQUEST,
    OBJ_MD_LISTENER
} object_kind_t;

typedef struct {
    char id[64];
    char name[96];
    char link[8];
    object_kind_t kind;
    UINT32 com_id;
    UINT8 *data;
    UINT32 data_len;
    TRDP_PUB_T pub;
    TRDP_SUB_T sub;
    TRDP_LIS_T listener;
    int active;
    int auto_reply;
} object_t;

typedef struct {
    TRDP_APP_SESSION_T app;
    char name;
    UINT32 own_ip;
    int active;
} link_t;

static link_t g_links[2];
static object_t g_objects[MAX_OBJECTS];
static volatile int g_running = 1;
static volatile int g_process_running = 0;
static bridge_thread_t g_process_thread;
static bridge_mutex_t g_lock;
static bridge_mutex_t g_out_lock;

/* ----- small cross-platform mutex wrapper ----- */
static void mutex_init(bridge_mutex_t *m) {
#ifdef _WIN32
    InitializeCriticalSection(m);
#else
    pthread_mutex_init(m, NULL);
#endif
}
static void mutex_lock(bridge_mutex_t *m) {
#ifdef _WIN32
    EnterCriticalSection(m);
#else
    pthread_mutex_lock(m);
#endif
}
static void mutex_unlock(bridge_mutex_t *m) {
#ifdef _WIN32
    LeaveCriticalSection(m);
#else
    pthread_mutex_unlock(m);
#endif
}

static void json_escape(FILE *f, const char *s) {
    const unsigned char *p = (const unsigned char *)s;
    for (; p && *p; ++p) {
        if (*p == '"' || *p == '\\') { fputc('\\', f); fputc(*p, f); }
        else if (*p == '\n') fputs("\\n", f);
        else if (*p == '\r') fputs("\\r", f);
        else if (*p == '\t') fputs("\\t", f);
        else if (*p >= 0x20) fputc(*p, f);
    }
}

static void emit_error(const char *message) {
    mutex_lock(&g_out_lock);
    fputs("{\"event\":\"error\",\"error\":\"", stdout);
    json_escape(stdout, message ? message : "unknown error");
    fputs("\"}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static void emit_ack(const char *command, const char *id) {
    mutex_lock(&g_out_lock);
    fputs("{\"event\":\"ack\",\"command\":\"", stdout);
    json_escape(stdout, command ? command : "");
    fputs("\"", stdout);
    if (id && *id) { fputs(",\"id\":\"", stdout); json_escape(stdout, id); fputs("\"", stdout); }
    fputs("}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static void print_ip(FILE *f, UINT32 ip) {
    fprintf(f, "%u.%u.%u.%u", (unsigned)((ip >> 24) & 0xffu), (unsigned)((ip >> 16) & 0xffu),
            (unsigned)((ip >> 8) & 0xffu), (unsigned)(ip & 0xffu));
}

static void print_hex(FILE *f, const UINT8 *data, UINT32 size) {
    static const char hex[] = "0123456789ABCDEF";
    UINT32 i;
    for (i = 0; data && i < size; ++i) {
        fputc(hex[(data[i] >> 4) & 0xf], f);
        fputc(hex[data[i] & 0xf], f);
    }
}

static const char *msg_name(TRDP_MSG_T msg) {
    switch (msg) {
        case TRDP_MSG_PD: return "Pd";
        case TRDP_MSG_PP: return "Pp";
        case TRDP_MSG_PR: return "Pr";
        case TRDP_MSG_PE: return "Pe";
        case TRDP_MSG_MN: return "Mn";
        case TRDP_MSG_MR: return "Mr";
        case TRDP_MSG_MP: return "Mp";
        case TRDP_MSG_MQ: return "Mq";
        case TRDP_MSG_MC: return "Mc";
        case TRDP_MSG_ME: return "Me";
        default: return "??";
    }
}

static char link_name_for_app(TRDP_APP_SESSION_T app) {
    if (g_links[1].active && g_links[1].app == app) return 'B';
    return 'A';
}

static void pd_callback(void *ref, TRDP_APP_SESSION_T app, const TRDP_PD_INFO_T *msg, UINT8 *data, UINT32 size) {
    object_t *obj = (object_t *)ref;
    if (!msg) return;
    mutex_lock(&g_out_lock);
    fprintf(stdout, "{\"event\":\"packet\",\"kind\":\"pd\",\"link\":\"%c\",\"id\":\"", link_name_for_app(app));
    json_escape(stdout, obj ? obj->id : "");
    fprintf(stdout, "\",\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,\"src_ip\":\"",
            msg_name(msg->msgType), (unsigned)msg->comId, (unsigned)msg->seqCount, (unsigned)msg->protVersion,
            (unsigned)msg->etbTopoCnt, (unsigned)msg->opTrnTopoCnt);
    print_ip(stdout, msg->srcIpAddr);
    fputs("\",\"dest_ip\":\"", stdout); print_ip(stdout, msg->destIpAddr);
    fprintf(stdout, "\",\"data_len\":%u,\"result_code\":%d,\"payload_hex\":\"", (unsigned)size, (int)msg->resultCode);
    print_hex(stdout, data, size); fputs("\"}\n", stdout); fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static TRDP_SEND_PARAM_T md_send_params(void) {
    TRDP_SEND_PARAM_T p;
    memset(&p, 0, sizeof(p));
    p.qos = 2u; p.ttl = 64u; p.retries = 2u;
    return p;
}

static void md_callback(void *ref, TRDP_APP_SESSION_T app, const TRDP_MD_INFO_T *msg, UINT8 *data, UINT32 size) {
    object_t *obj = (object_t *)ref;
    if (!msg) return;
    mutex_lock(&g_out_lock);
    fprintf(stdout, "{\"event\":\"packet\",\"kind\":\"md\",\"link\":\"%c\",\"id\":\"", link_name_for_app(app));
    json_escape(stdout, obj ? obj->id : "");
    fprintf(stdout, "\",\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,\"src_ip\":\"",
            msg_name(msg->msgType), (unsigned)msg->comId, (unsigned)msg->seqCount, (unsigned)msg->protVersion,
            (unsigned)msg->etbTopoCnt, (unsigned)msg->opTrnTopoCnt);
    print_ip(stdout, msg->srcIpAddr); fputs("\",\"dest_ip\":\"", stdout); print_ip(stdout, msg->destIpAddr);
    fprintf(stdout, "\",\"data_len\":%u,\"result_code\":%d,\"reply_status\":%d,\"user_status\":%u,\"num_replies\":%u,\"payload_hex\":\"",
            (unsigned)size, (int)msg->resultCode, (int)msg->replyStatus, (unsigned)msg->userStatus, (unsigned)msg->numReplies);
    print_hex(stdout, data, size); fputs("\"}\n", stdout); fflush(stdout);
    mutex_unlock(&g_out_lock);

    if (obj && obj->active && obj->auto_reply && msg->msgType == TRDP_MSG_MR) {
        TRDP_SEND_PARAM_T send = md_send_params();
        TRDP_ERR_T err = tlm_reply(app, &msg->sessionId, msg->comId, 0u, &send, obj->data, obj->data_len, NULL);
        if (err != TRDP_NO_ERR) emit_error("TCNOpen tlm_reply failed");
    }
}

/* ----- restricted JSON accessors ----- */
static const char *find_key(const char *line, const char *key) {
    static char needle[96];
    const char *p;
    snprintf(needle, sizeof(needle), "\"%s\"", key);
    p = strstr(line, needle);
    if (!p) return NULL;
    p += strlen(needle);
    while (*p && isspace((unsigned char)*p)) ++p;
    if (*p++ != ':') return NULL;
    while (*p && isspace((unsigned char)*p)) ++p;
    return p;
}

static int jstr(const char *line, const char *key, char *out, size_t cap) {
    const char *p = find_key(line, key); size_t n = 0;
    if (!p || *p != '"' || cap == 0) return 0;
    ++p;
    while (*p && *p != '"' && n + 1 < cap) {
        if (*p == '\\' && p[1]) { ++p; if (*p == 'n') out[n++] = '\n'; else if (*p == 'r') out[n++] = '\r'; else if (*p == 't') out[n++] = '\t'; else out[n++] = *p; ++p; }
        else out[n++] = *p++;
    }
    out[n] = 0;
    return *p == '"';
}

static uint32_t ju32(const char *line, const char *key, uint32_t fallback) {
    const char *p = find_key(line, key); char *end = NULL; unsigned long v;
    if (!p) return fallback;
    v = strtoul(p, &end, 10);
    return end == p ? fallback : (uint32_t)v;
}

static int jbool(const char *line, const char *key, int fallback) {
    const char *p = find_key(line, key);
    if (!p) return fallback;
    if (!strncmp(p, "true", 4)) return 1;
    if (!strncmp(p, "false", 5)) return 0;
    return fallback;
}

static UINT8 *hex_decode(const char *text, UINT32 *size) {
    size_t len, i; UINT8 *out;
    *size = 0u; if (!text || !(len = strlen(text))) return NULL;
    if (len & 1u) return NULL;
    out = (UINT8 *)malloc(len / 2u); if (!out) return NULL;
    for (i = 0; i < len; i += 2u) {
        char tmp[3] = {text[i], text[i + 1], 0}; char *end = NULL;
        unsigned long v = strtoul(tmp, &end, 16);
        if (!end || *end) { free(out); return NULL; }
        out[i / 2u] = (UINT8)v;
    }
    *size = (UINT32)(len / 2u); return out;
}

static object_t *alloc_object(const char *id) {
    int i;
    for (i = 0; i < MAX_OBJECTS; ++i) if (!g_objects[i].active) {
        memset(&g_objects[i], 0, sizeof(g_objects[i]));
        strncpy(g_objects[i].id, id, sizeof(g_objects[i].id) - 1u);
        g_objects[i].active = 1; return &g_objects[i];
    }
    return NULL;
}
static object_t *find_object(const char *id) {
    int i; for (i = 0; i < MAX_OBJECTS; ++i) if (g_objects[i].active && !strcmp(g_objects[i].id, id)) return &g_objects[i];
    return NULL;
}
static link_t *get_link(const char *name) {
    if (name && (!strcmp(name, "b") || !strcmp(name, "B"))) return g_links[1].active ? &g_links[1] : NULL;
    return g_links[0].active ? &g_links[0] : NULL;
}

static TRDP_ERR_T open_link(link_t *link, char name, const char *ip) {
    TRDP_PD_CONFIG_T pd; TRDP_MD_CONFIG_T md;
    memset(&pd, 0, sizeof(pd)); memset(&md, 0, sizeof(md));
    pd.flags = TRDP_FLAGS_CALLBACK; pd.timeout = 100000u; pd.toBehavior = TRDP_TO_SET_TO_ZERO; pd.port = TRDP_PD_PORT_TAU;
    pd.sendParam.qos = 2u; pd.sendParam.ttl = 64u;
    md.flags = TRDP_FLAGS_CALLBACK; md.replyTimeout = 5000000u; md.confirmTimeout = 1000000u; md.connectTimeout = 60000000u;
    md.udpPort = TRDP_MD_PORT_TAU; md.tcpPort = TRDP_MD_PORT_TAU; md.sendParam = md_send_params(); md.maxNumSessions = 64u;
    link->name = name; link->own_ip = vos_dottedIP(ip && *ip ? ip : "0.0.0.0");
    if (tlc_openSession(&link->app, link->own_ip, 0u, NULL, &pd, &md, NULL) != TRDP_NO_ERR) return TRDP_INIT_ERR;
    link->active = 1; return TRDP_NO_ERR;
}

#ifdef _WIN32
static unsigned __stdcall process_loop(void *unused)
#else
static void *process_loop(void *unused)
#endif
{
    (void)unused; g_process_running = 1;
    while (g_running) {
        int i;
        mutex_lock(&g_lock);
        for (i = 0; i < 2; ++i) if (g_links[i].active) {
            TRDP_FDS_T rfds; TRDP_TIME_T tv; TRDP_SOCK_T no_desc = 0; INT32 ready;
            FD_ZERO(&rfds);
            if (tlc_getInterval(g_links[i].app, &tv, &rfds, &no_desc) == TRDP_NO_ERR) {
                if (tv.tv_sec > 0 || tv.tv_usec > 10000) { tv.tv_sec = 0; tv.tv_usec = 10000; }
                ready = vos_select(no_desc + 1, &rfds, NULL, NULL, &tv);
                if (ready < 0) ready = 0;
                (void)tlc_process(g_links[i].app, &rfds, &ready);
            }
        }
        mutex_unlock(&g_lock);
        SLEEP_MS(1);
    }
    g_process_running = 0;
#ifdef _WIN32
    return 0;
#else
    return NULL;
#endif
}

static int start_process_thread(void) {
#ifdef _WIN32
    uintptr_t h = _beginthreadex(NULL, 0, process_loop, NULL, 0, NULL);
    if (!h) return 0; g_process_thread = (HANDLE)h;
#else
    if (pthread_create(&g_process_thread, NULL, process_loop, NULL) != 0) return 0;
#endif
    return 1;
}

static void stop_object(object_t *obj) {
    int i;
    if (!obj || !obj->active) return;
    mutex_lock(&g_lock);
    for (i = 0; i < 2; ++i) if (g_links[i].active && (!strcmp(obj->link, "both") || (obj->link[0] | 32) == ('a' + i))) {
        if (obj->kind == OBJ_PD_PUBLISHER && obj->pub) (void)tlp_unpublish(g_links[i].app, obj->pub);
        if ((obj->kind == OBJ_PD_SUBSCRIBER || obj->kind == OBJ_PD_REQUEST) && obj->sub) (void)tlp_unsubscribe(g_links[i].app, obj->sub);
        if (obj->kind == OBJ_MD_LISTENER && obj->listener) (void)tlm_delListener(g_links[i].app, obj->listener);
    }
    mutex_unlock(&g_lock);
    free(obj->data); memset(obj, 0, sizeof(*obj));
}

static TRDP_ERR_T start_on_link(object_t *obj, link_t *link, const char *kind, const char *dest, const char *source,
                                UINT32 cycle, const char *transport) {
    TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB;
    TRDP_IP_ADDR_T dst = vos_dottedIP(dest && *dest ? dest : "0.0.0.0");
    TRDP_IP_ADDR_T src = vos_dottedIP(source && *source ? source : "0.0.0.0");
    if (!strcmp(kind, "pd_publisher")) {
        obj->kind = OBJ_PD_PUBLISHER;
        return tlp_publish(link->app, &obj->pub, obj, pd_callback, 0u, obj->com_id, 0u, 0u, src, dst,
                           cycle ? cycle : 100000u, 0u, flags, obj->data, obj->data_len);
    }
    if (!strcmp(kind, "pd_subscriber") || !strcmp(kind, "pd_request")) {
        TRDP_ERR_T err; obj->kind = !strcmp(kind, "pd_request") ? OBJ_PD_REQUEST : OBJ_PD_SUBSCRIBER;
        err = tlp_subscribe(link->app, &obj->sub, obj, pd_callback, 0u, obj->com_id, 0u, 0u, src, 0u, dst,
                            flags, cycle ? cycle : 100000u, TRDP_TO_KEEP_LAST_VALUE);
        if (err != TRDP_NO_ERR || obj->kind != OBJ_PD_REQUEST) return err;
        return tlp_request(link->app, obj->sub, 0u, obj->com_id, 0u, 0u, src, dst, 0u, flags,
                           obj->data, obj->data_len, obj->com_id, src);
    }
    if (!strcmp(kind, "md_listener")) {
        obj->kind = OBJ_MD_LISTENER; obj->auto_reply = 1;
        if (transport && !strcmp(transport, "tcp")) flags |= TRDP_FLAGS_TCP;
        return tlm_addListener(link->app, &obj->listener, obj, md_callback, TRUE, obj->com_id, 0u, 0u,
                               src, 0u, dst, flags, NULL, NULL);
    }
    return TRDP_PARAM_ERR;
}

static void handle_object_start(const char *line) {
    char id[64] = {0}, kind[32] = {0}, name[96] = {0}, link_name[8] = "a", dest[64] = {0}, source[64] = {0}, payload[MAX_PAYLOAD * 2 + 1] = {0}, transport[16] = "udp";
    UINT32 cycle, com_id; object_t *obj; TRDP_ERR_T err = TRDP_NO_ERR; int i, started = 0;
    if (!jstr(line, "id", id, sizeof(id)) || !jstr(line, "kind", kind, sizeof(kind))) { emit_error("object_start requires id and kind"); return; }
    (void)jstr(line, "name", name, sizeof(name)); (void)jstr(line, "link", link_name, sizeof(link_name));
    (void)jstr(line, "destination", dest, sizeof(dest)); (void)jstr(line, "source", source, sizeof(source));
    (void)jstr(line, "payload_hex", payload, sizeof(payload)); (void)jstr(line, "transport", transport, sizeof(transport));
    com_id = ju32(line, "com_id", 0u); cycle = ju32(line, "cycle_us", 100000u);
    if (!com_id) { emit_error("ComID must be non-zero"); return; }

    /* one-shot MD operations do not allocate a persistent object */
    if (!strcmp(kind, "md_request") || !strcmp(kind, "md_notify")) {
        UINT32 size = 0u; UINT8 *data = hex_decode(payload, &size); TRDP_SEND_PARAM_T send = md_send_params();
        TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK; TRDP_UUID_T session; link_t *link = get_link(link_name);
        if (!link) { free(data); emit_error("selected TRDP link is not active"); return; }
        if (!strcmp(transport, "tcp")) flags |= TRDP_FLAGS_TCP;
        mutex_lock(&g_lock);
        if (!strcmp(kind, "md_request")) err = tlm_request(link->app, NULL, md_callback, &session, com_id, 0u, 0u, 0u,
            vos_dottedIP(dest), flags, 1u, 5000000u, &send, data, size, NULL, NULL);
        else err = tlm_notify(link->app, NULL, md_callback, com_id, 0u, 0u, 0u, vos_dottedIP(dest), flags, &send, data, size, NULL, NULL);
        mutex_unlock(&g_lock); free(data);
        if (err != TRDP_NO_ERR) emit_error("TCNOpen MD operation failed"); else emit_ack(kind, id);
        return;
    }

    if (find_object(id)) { emit_error("object id already active"); return; }
    obj = alloc_object(id); if (!obj) { emit_error("too many TRDP objects"); return; }
    strncpy(obj->name, name, sizeof(obj->name) - 1u); strncpy(obj->link, link_name, sizeof(obj->link) - 1u); obj->com_id = com_id;
    obj->data = hex_decode(payload, &obj->data_len);
    mutex_lock(&g_lock);
    for (i = 0; i < 2; ++i) {
        if (!g_links[i].active) continue;
        if (strcmp(link_name, "both") && (link_name[0] | 32) != ('a' + i)) continue;
        err = start_on_link(obj, &g_links[i], kind, dest, source, cycle, transport); if (err != TRDP_NO_ERR) break; ++started;
    }
    mutex_unlock(&g_lock);
    if (err != TRDP_NO_ERR || !started) { stop_object(obj); emit_error("TCNOpen object_start failed"); return; }
    emit_ack("object_start", id);
}

/* ----- dynamic libpcap/Npcap live capture ----- */
typedef struct pcap pcap_t;
typedef unsigned int bpf_u_int32;
struct tau_pcap_pkthdr { struct timeval ts; bpf_u_int32 caplen; bpf_u_int32 len; };
struct bpf_insn_tau { unsigned short code; unsigned char jt; unsigned char jf; bpf_u_int32 k; };
struct bpf_program_tau { unsigned int bf_len; struct bpf_insn_tau *bf_insns; };
typedef pcap_t *(*fn_pcap_open_live)(const char *, int, int, int, char *);
typedef int (*fn_pcap_next_ex)(pcap_t *, struct tau_pcap_pkthdr **, const unsigned char **);
typedef void (*fn_pcap_close)(pcap_t *);
typedef int (*fn_pcap_compile)(pcap_t *, struct bpf_program_tau *, const char *, int, bpf_u_int32);
typedef int (*fn_pcap_setfilter)(pcap_t *, struct bpf_program_tau *);
typedef void (*fn_pcap_freecode)(struct bpf_program_tau *);

static fn_pcap_open_live dyn_open_live;
static fn_pcap_next_ex dyn_next_ex;
static fn_pcap_close dyn_close;
static fn_pcap_compile dyn_compile;
static fn_pcap_setfilter dyn_setfilter;
static fn_pcap_freecode dyn_freecode;
static void *g_pcap_lib;
static pcap_t *g_pcap;
static volatile int g_capture_running;
static bridge_thread_t g_capture_thread;

static void *dyn_symbol(const char *name) {
#ifdef _WIN32
    return (void *)GetProcAddress((HMODULE)g_pcap_lib, name);
#else
    return dlsym(g_pcap_lib, name);
#endif
}
static int load_pcap(void) {
    if (g_pcap_lib) return 1;
#ifdef _WIN32
    g_pcap_lib = (void *)LoadLibraryA("wpcap.dll");
#else
#ifdef __APPLE__
    g_pcap_lib = dlopen("/usr/lib/libpcap.A.dylib", RTLD_NOW);
    if (!g_pcap_lib) g_pcap_lib = dlopen("libpcap.dylib", RTLD_NOW);
#else
    g_pcap_lib = dlopen("libpcap.so.1", RTLD_NOW);
    if (!g_pcap_lib) g_pcap_lib = dlopen("libpcap.so", RTLD_NOW);
#endif
#endif
    if (!g_pcap_lib) return 0;
    dyn_open_live = (fn_pcap_open_live)dyn_symbol("pcap_open_live"); dyn_next_ex = (fn_pcap_next_ex)dyn_symbol("pcap_next_ex");
    dyn_close = (fn_pcap_close)dyn_symbol("pcap_close"); dyn_compile = (fn_pcap_compile)dyn_symbol("pcap_compile");
    dyn_setfilter = (fn_pcap_setfilter)dyn_symbol("pcap_setfilter"); dyn_freecode = (fn_pcap_freecode)dyn_symbol("pcap_freecode");
    return dyn_open_live && dyn_next_ex && dyn_close && dyn_compile && dyn_setfilter && dyn_freecode;
}

static uint16_t rd16(const unsigned char *p) { return (uint16_t)(((uint16_t)p[0] << 8) | p[1]); }
static uint32_t rd32(const unsigned char *p) { return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3]; }
static void capture_emit(const struct tau_pcap_pkthdr *h, const unsigned char *frame) {
    size_t ip = 14, ihl, l4, payload; uint16_t et, sport, dport; unsigned char proto; uint32_t data_len; char mt[3];
    if (!h || !frame || h->caplen < 14 + 20) return;
    et = rd16(frame + 12); if (et == 0x8100 || et == 0x88a8) { if (h->caplen < 18 + 20) return; et = rd16(frame + 16); ip = 18; }
    if (et != 0x0800 || (frame[ip] >> 4) != 4) return; ihl = (frame[ip] & 15u) * 4u; if (ihl < 20 || ip + ihl + 8 > h->caplen) return;
    proto = frame[ip + 9]; l4 = ip + ihl;
    if (proto == 17) { sport = rd16(frame + l4); dport = rd16(frame + l4 + 2); payload = l4 + 8; }
    else if (proto == 6) { size_t th = (frame[l4 + 12] >> 4) * 4u; sport = rd16(frame + l4); dport = rd16(frame + l4 + 2); payload = l4 + th; }
    else return;
    if (sport != TRDP_PD_PORT_TAU && dport != TRDP_PD_PORT_TAU && sport != TRDP_MD_PORT_TAU && dport != TRDP_MD_PORT_TAU) return;
    if (payload + 24 > h->caplen) return; mt[0] = (char)frame[payload + 6]; mt[1] = (char)frame[payload + 7]; mt[2] = 0; if (!isalpha((unsigned char)mt[0]) || !isalpha((unsigned char)mt[1])) return;
    data_len = rd32(frame + payload + 20);
    mutex_lock(&g_out_lock);
    fprintf(stdout, "{\"event\":\"packet\",\"kind\":\"capture\",\"link\":\"capture\",\"timestamp_us\":%llu,\"transport\":\"%s\",\"src_port\":%u,\"dest_port\":%u,\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,\"src_ip\":\"%u.%u.%u.%u\",\"dest_ip\":\"%u.%u.%u.%u\",\"data_len\":%u,\"raw_frame_hex\":\"",
        (unsigned long long)h->ts.tv_sec * 1000000ull + (unsigned long long)h->ts.tv_usec, proto == 17 ? "udp" : "tcp", (unsigned)sport, (unsigned)dport, mt,
        (unsigned)rd32(frame + payload + 8), (unsigned)rd32(frame + payload), (unsigned)rd16(frame + payload + 4), (unsigned)rd32(frame + payload + 12), (unsigned)rd32(frame + payload + 16),
        frame[ip+12],frame[ip+13],frame[ip+14],frame[ip+15],frame[ip+16],frame[ip+17],frame[ip+18],frame[ip+19],(unsigned)data_len);
    print_hex(stdout, frame, h->caplen); fputs("\"}\n", stdout); fflush(stdout); mutex_unlock(&g_out_lock);
}
#ifdef _WIN32
static unsigned __stdcall capture_loop(void *unused)
#else
static void *capture_loop(void *unused)
#endif
{
    (void)unused;
    while (g_capture_running && g_pcap) { struct tau_pcap_pkthdr *h = NULL; const unsigned char *data = NULL; int rc = dyn_next_ex(g_pcap, &h, &data); if (rc == 1) capture_emit(h, data); else if (rc < 0) break; }
    g_capture_running = 0;
#ifdef _WIN32
    return 0;
#else
    return NULL;
#endif
}
static void capture_stop(void) {
    g_capture_running = 0; SLEEP_MS(150);
    if (g_pcap) { dyn_close(g_pcap); g_pcap = NULL; }
}
static void capture_start(const char *line) {
    char iface[512] = {0}, filter[1024] = "udp port 17224 or udp port 17225 or tcp port 17225", errbuf[256] = {0}; struct bpf_program_tau prog;
    if (!jstr(line, "interface", iface, sizeof(iface)) || !*iface) { emit_error("live capture requires interface name"); return; }
    (void)jstr(line, "filter", filter, sizeof(filter)); if (!load_pcap()) { emit_error("libpcap/Npcap not found. Windows users must install Npcap separately."); return; }
    capture_stop(); g_pcap = dyn_open_live(iface, 65535, 1, 100, errbuf); if (!g_pcap) { emit_error(*errbuf ? errbuf : "pcap_open_live failed"); return; }
    memset(&prog, 0, sizeof(prog)); if (dyn_compile(g_pcap, &prog, filter, 1, 0xffffffffu) != 0 || dyn_setfilter(g_pcap, &prog) != 0) { dyn_freecode(&prog); capture_stop(); emit_error("pcap capture filter failed"); return; } dyn_freecode(&prog);
    g_capture_running = 1;
#ifdef _WIN32
    { uintptr_t h = _beginthreadex(NULL, 0, capture_loop, NULL, 0, NULL); if (!h) { capture_stop(); emit_error("capture thread failed"); return; } g_capture_thread = (HANDLE)h; }
#else
    if (pthread_create(&g_capture_thread, NULL, capture_loop, NULL) != 0) { capture_stop(); emit_error("capture thread failed"); return; }
#endif
    emit_ack("capture_start", NULL);
}

static void bridge_shutdown(void) {
    int i; capture_stop();
    for (i = 0; i < MAX_OBJECTS; ++i) if (g_objects[i].active) stop_object(&g_objects[i]);
    g_running = 0; SLEEP_MS(30);
    mutex_lock(&g_lock);
    for (i = 0; i < 2; ++i) if (g_links[i].active) { (void)tlc_closeSession(g_links[i].app); g_links[i].active = 0; }
    mutex_unlock(&g_lock); (void)tlc_terminate();
}

static void handle_open(const char *line) {
    char a[64] = "0.0.0.0", b[64] = "0.0.0.0"; int b_enabled = jbool(line, "link_b_enabled", 0); TRDP_ERR_T err;
    (void)jstr(line, "link_a_ip", a, sizeof(a)); (void)jstr(line, "link_b_ip", b, sizeof(b));
    if (tlc_init(NULL, NULL, NULL) != TRDP_NO_ERR) { emit_error("TCNOpen tlc_init failed"); return; }
    mutex_lock(&g_lock); err = open_link(&g_links[0], 'A', a); if (err == TRDP_NO_ERR && b_enabled) err = open_link(&g_links[1], 'B', b); mutex_unlock(&g_lock);
    if (err != TRDP_NO_ERR) { emit_error("TCNOpen tlc_openSession failed; verify local interface IPv4"); return; }
    if (!start_process_thread()) { emit_error("TRDP process thread failed"); return; }
    emit_ack("open", NULL);
}

int main(void) {
    char *line = (char *)malloc(MAX_LINE); if (!line) return 2;
    mutex_init(&g_lock); mutex_init(&g_out_lock); memset(g_links, 0, sizeof(g_links)); memset(g_objects, 0, sizeof(g_objects));
    while (g_running && fgets(line, MAX_LINE, stdin)) {
        char command[64] = {0}, id[64] = {0};
        if (!jstr(line, "command", command, sizeof(command))) { emit_error("missing command"); continue; }
        if (!strcmp(command, "open")) handle_open(line);
        else if (!strcmp(command, "object_start")) handle_object_start(line);
        else if (!strcmp(command, "object_stop")) { if (!jstr(line, "id", id, sizeof(id))) emit_error("object_stop requires id"); else { object_t *obj = find_object(id); if (obj) stop_object(obj); emit_ack("object_stop", id); } }
        else if (!strcmp(command, "capture_start")) capture_start(line);
        else if (!strcmp(command, "capture_stop")) { capture_stop(); emit_ack("capture_stop", NULL); }
        else if (!strcmp(command, "shutdown")) { emit_ack("shutdown", NULL); break; }
        else emit_error("unknown TRDP bridge command");
    }
    bridge_shutdown(); free(line); return 0;
}
