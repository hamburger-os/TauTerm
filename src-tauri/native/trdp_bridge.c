/*
 * TauTerm TRDP native bridge
 *
 * TauTerm code in this file is MIT OR Apache-2.0. It links against TCNOpen
 * TRDP 3.0.0.0 (MPL-2.0) as a separate native component. No TCNOpen source
 * is copied into this file.
 *
 * IPC: newline-delimited JSON on stdin/stdout. The intentionally small parser
 * accepts the compact command envelopes emitted by TauTerm.
 */

#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif

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
#include <winsock2.h>
#include <windows.h>
#include <process.h>
typedef HANDLE bridge_thread_t;
typedef CRITICAL_SECTION bridge_mutex_t;
#else
#include <dlfcn.h>
#include <pthread.h>
#include <sys/time.h>
typedef pthread_t bridge_thread_t;
typedef pthread_mutex_t bridge_mutex_t;
#endif

#define MAX_OBJECTS 256
#define MAX_PAYLOAD 65536
#define MAX_LINE 131072
#define DEFAULT_PD_PORT 17224u
#define DEFAULT_MD_PORT 17225u

#define LINKTYPE_NULL 0
#define LINKTYPE_ETHERNET 1
#define LINKTYPE_RAW 101
#define LINKTYPE_LINUX_SLL 113
#define LINKTYPE_LINUX_SLL2 276

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
    UINT32 etb_topo_count;
    UINT32 op_trn_topo_count;
    UINT32 red_id;
    BOOL8 red_leader;
    TRDP_TO_BEHAVIOR_T timeout_behavior;
    UINT8 *data;
    UINT32 data_len;
    TRDP_PUB_T pub[2];
    TRDP_SUB_T sub[2];
    TRDP_LIS_T listener[2];
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
static int g_tlc_initialized = 0;

static bridge_thread_t g_process_thread;
static int g_process_thread_active = 0;
static bridge_mutex_t g_lock;
static bridge_mutex_t g_out_lock;

/* ---------- platform helpers ---------- */

static void mutex_init(bridge_mutex_t *mutex) {
#ifdef _WIN32
    InitializeCriticalSection(mutex);
#else
    (void)pthread_mutex_init(mutex, NULL);
#endif
}

static void mutex_lock(bridge_mutex_t *mutex) {
#ifdef _WIN32
    EnterCriticalSection(mutex);
#else
    (void)pthread_mutex_lock(mutex);
#endif
}

static void mutex_unlock(bridge_mutex_t *mutex) {
#ifdef _WIN32
    LeaveCriticalSection(mutex);
#else
    (void)pthread_mutex_unlock(mutex);
#endif
}

static void sleep_ms(unsigned int milliseconds) {
#ifdef _WIN32
    Sleep((DWORD)milliseconds);
#else
    struct timespec value;
    value.tv_sec = (time_t)(milliseconds / 1000u);
    value.tv_nsec = (long)(milliseconds % 1000u) * 1000000L;
    (void)nanosleep(&value, NULL);
#endif
}

static void thread_join(bridge_thread_t thread) {
#ifdef _WIN32
    (void)WaitForSingleObject(thread, INFINITE);
    (void)CloseHandle(thread);
#else
    (void)pthread_join(thread, NULL);
#endif
}

/* ---------- JSON output ---------- */

static void json_escape(FILE *file, const char *text) {
    const unsigned char *cursor = (const unsigned char *)text;
    while (cursor != NULL && *cursor != 0u) {
        if (*cursor == '"' || *cursor == '\\') {
            fputc('\\', file);
            fputc((int)*cursor, file);
        } else if (*cursor == '\n') {
            fputs("\\n", file);
        } else if (*cursor == '\r') {
            fputs("\\r", file);
        } else if (*cursor == '\t') {
            fputs("\\t", file);
        } else if (*cursor >= 0x20u) {
            fputc((int)*cursor, file);
        }
        ++cursor;
    }
}

static void emit_error(const char *message) {
    mutex_lock(&g_out_lock);
    fputs("{\"event\":\"error\",\"error\":\"", stdout);
    json_escape(stdout, message != NULL ? message : "unknown error");
    fputs("\"}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static void emit_ack(const char *command, const char *id) {
    mutex_lock(&g_out_lock);
    fputs("{\"event\":\"ack\",\"command\":\"", stdout);
    json_escape(stdout, command != NULL ? command : "");
    fputs("\"", stdout);
    if (id != NULL && *id != '\0') {
        fputs(",\"id\":\"", stdout);
        json_escape(stdout, id);
        fputs("\"", stdout);
    }
    fputs("}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static void print_ip(FILE *file, UINT32 ip) {
    const CHAR8 *text = vos_ipDotted(ip);
    fputs(text != NULL ? text : "0.0.0.0", file);
}

static void print_hex(FILE *file, const UINT8 *data, UINT32 size) {
    static const char digits[] = "0123456789ABCDEF";
    UINT32 index;
    for (index = 0u; data != NULL && index < size; ++index) {
        fputc(digits[(data[index] >> 4) & 0x0fu], file);
        fputc(digits[data[index] & 0x0fu], file);
    }
}

static const char *message_name(TRDP_MSG_T message) {
    switch (message) {
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
    if (g_links[1].active && g_links[1].app == app) {
        return 'B';
    }
    return 'A';
}

static void pd_callback(
    void *ref,
    TRDP_APP_SESSION_T app,
    const TRDP_PD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    object_t *object = (object_t *)ref;
    if (message == NULL) {
        return;
    }

    mutex_lock(&g_out_lock);
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"pd\",\"link\":\"%c\",\"id\":\"",
        link_name_for_app(app)
    );
    json_escape(stdout, object != NULL ? object->id : "");
    fprintf(
        stdout,
        "\",\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,"
        "\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,"
        "\"src_ip\":\"",
        message_name(message->msgType),
        (unsigned int)message->comId,
        (unsigned int)message->seqCount,
        (unsigned int)message->protVersion,
        (unsigned int)message->etbTopoCnt,
        (unsigned int)message->opTrnTopoCnt
    );
    print_ip(stdout, message->srcIpAddr);
    fputs("\",\"dest_ip\":\"", stdout);
    print_ip(stdout, message->destIpAddr);
    fprintf(
        stdout,
        "\",\"data_len\":%u,\"result_code\":%d,\"payload_hex\":\"",
        (unsigned int)size,
        (int)message->resultCode
    );
    print_hex(stdout, data, size);
    fputs("\"}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

static TRDP_COM_PARAM_T md_send_params(void) {
    TRDP_COM_PARAM_T send;
    memset(&send, 0, sizeof(send));
    send.qos = 2u;
    send.ttl = 64u;
    send.retries = 2u;
    return send;
}

static void md_callback(
    void *ref,
    TRDP_APP_SESSION_T app,
    const TRDP_MD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    object_t *object = (object_t *)ref;
    if (message == NULL) {
        return;
    }

    mutex_lock(&g_out_lock);
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"md\",\"link\":\"%c\",\"id\":\"",
        link_name_for_app(app)
    );
    json_escape(stdout, object != NULL ? object->id : "");
    fprintf(
        stdout,
        "\",\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,"
        "\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,"
        "\"src_ip\":\"",
        message_name(message->msgType),
        (unsigned int)message->comId,
        (unsigned int)message->seqCount,
        (unsigned int)message->protVersion,
        (unsigned int)message->etbTopoCnt,
        (unsigned int)message->opTrnTopoCnt
    );
    print_ip(stdout, message->srcIpAddr);
    fputs("\",\"dest_ip\":\"", stdout);
    print_ip(stdout, message->destIpAddr);
    fprintf(
        stdout,
        "\",\"data_len\":%u,\"result_code\":%d,\"reply_status\":%d,"
        "\"user_status\":%u,\"num_replies\":%u,\"payload_hex\":\"",
        (unsigned int)size,
        (int)message->resultCode,
        (int)message->replyStatus,
        (unsigned int)message->userStatus,
        (unsigned int)message->numReplies
    );
    print_hex(stdout, data, size);
    fputs("\"}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);

    if (
        object != NULL
        && object->active
        && object->auto_reply
        && message->msgType == TRDP_MSG_MR
    ) {
        TRDP_COM_PARAM_T send = md_send_params();
        TRDP_ERR_T error = tlm_reply(
            app,
            &message->sessionId,
            message->comId,
            0u,
            &send,
            object->data,
            object->data_len,
            NULL
        );
        if (error != TRDP_NO_ERR) {
            emit_error("TCNOpen tlm_reply failed");
        }
    }
}

/* ---------- restricted JSON accessors ---------- */

static const char *find_key(const char *line, const char *key) {
    static char needle[96];
    const char *cursor;
    (void)snprintf(needle, sizeof(needle), "\"%s\"", key);
    cursor = strstr(line, needle);
    if (cursor == NULL) {
        return NULL;
    }
    cursor += strlen(needle);
    while (*cursor != '\0' && isspace((unsigned char)*cursor)) {
        ++cursor;
    }
    if (*cursor != ':') {
        return NULL;
    }
    ++cursor;
    while (*cursor != '\0' && isspace((unsigned char)*cursor)) {
        ++cursor;
    }
    return cursor;
}

static int json_string(
    const char *line,
    const char *key,
    char *output,
    size_t capacity,
    const char *fallback
) {
    const char *cursor = find_key(line, key);
    size_t length = 0u;

    if (cursor == NULL || *cursor != '"' || capacity == 0u) {
        if (fallback != NULL && capacity > 0u) {
            (void)snprintf(output, capacity, "%s", fallback);
            return 1;
        }
        return 0;
    }

    ++cursor;
    while (*cursor != '\0' && *cursor != '"' && length + 1u < capacity) {
        if (*cursor == '\\' && cursor[1] != '\0') {
            ++cursor;
            if (*cursor == 'n') {
                output[length++] = '\n';
            } else if (*cursor == 'r') {
                output[length++] = '\r';
            } else if (*cursor == 't') {
                output[length++] = '\t';
            } else {
                output[length++] = *cursor;
            }
        } else {
            output[length++] = *cursor;
        }
        ++cursor;
    }
    output[length] = '\0';
    return *cursor == '"';
}

static uint32_t json_u32(const char *line, const char *key, uint32_t fallback) {
    const char *cursor = find_key(line, key);
    char *end = NULL;
    unsigned long value;
    if (cursor == NULL) {
        return fallback;
    }
    value = strtoul(cursor, &end, 10);
    return end == cursor ? fallback : (uint32_t)value;
}

static int json_bool(const char *line, const char *key, int fallback) {
    const char *cursor = find_key(line, key);
    if (cursor == NULL) {
        return fallback;
    }
    if (strncmp(cursor, "true", 4u) == 0) {
        return 1;
    }
    if (strncmp(cursor, "false", 5u) == 0) {
        return 0;
    }
    return fallback;
}

static int hex_value(char character) {
    if (character >= '0' && character <= '9') {
        return character - '0';
    }
    if (character >= 'a' && character <= 'f') {
        return character - 'a' + 10;
    }
    if (character >= 'A' && character <= 'F') {
        return character - 'A' + 10;
    }
    return -1;
}

static UINT8 *hex_decode(const char *text, UINT32 *size) {
    size_t length;
    size_t index;
    UINT8 *output;

    *size = 0u;
    if (text == NULL || *text == '\0') {
        return NULL;
    }
    length = strlen(text);
    if ((length & 1u) != 0u || length / 2u > MAX_PAYLOAD) {
        return NULL;
    }

    output = (UINT8 *)malloc(length / 2u);
    if (output == NULL) {
        return NULL;
    }
    for (index = 0u; index < length; index += 2u) {
        int high = hex_value(text[index]);
        int low = hex_value(text[index + 1u]);
        if (high < 0 || low < 0) {
            free(output);
            return NULL;
        }
        output[index / 2u] = (UINT8)((high << 4) | low);
    }
    *size = (UINT32)(length / 2u);
    return output;
}

static object_t *allocate_object(const char *id) {
    int index;
    for (index = 0; index < MAX_OBJECTS; ++index) {
        if (!g_objects[index].active) {
            memset(&g_objects[index], 0, sizeof(g_objects[index]));
            (void)snprintf(g_objects[index].id, sizeof(g_objects[index].id), "%s", id);
            g_objects[index].active = 1;
            g_objects[index].red_leader = TRUE;
            g_objects[index].timeout_behavior = TRDP_TO_KEEP_LAST_VALUE;
            return &g_objects[index];
        }
    }
    return NULL;
}

static object_t *find_object(const char *id) {
    int index;
    for (index = 0; index < MAX_OBJECTS; ++index) {
        if (g_objects[index].active && strcmp(g_objects[index].id, id) == 0) {
            return &g_objects[index];
        }
    }
    return NULL;
}

static int link_selected(const char *selection, int index) {
    if (selection == NULL || *selection == '\0') {
        return index == 0;
    }
    if (strcmp(selection, "both") == 0) {
        return 1;
    }
    if (index == 0) {
        return selection[0] == 'a' || selection[0] == 'A';
    }
    return selection[0] == 'b' || selection[0] == 'B';
}

/* ---------- TCNOpen session and object lifecycle ---------- */

static TRDP_ERR_T open_link(
    link_t *link,
    char name,
    const char *ip,
    UINT16 pd_port,
    UINT16 md_udp_port,
    UINT16 md_tcp_port
) {
    TRDP_PD_CONFIG_T pd;
    TRDP_MD_CONFIG_T md;

    memset(&pd, 0, sizeof(pd));
    memset(&md, 0, sizeof(md));

    pd.flags = TRDP_FLAGS_CALLBACK;
    pd.timeout = TRDP_DEFAULT_PD_TIMEOUT;
    pd.toBehavior = TRDP_TO_SET_TO_ZERO;
    pd.port = pd_port;
    pd.sendParam.qos = 2u;
    pd.sendParam.ttl = 64u;

    md.flags = TRDP_FLAGS_CALLBACK;
    md.replyTimeout = 5000000u;
    md.confirmTimeout = 1000000u;
    md.connectTimeout = 60000000u;
    md.sendingTimeout = 5000000u;
    md.udpPort = md_udp_port;
    md.tcpPort = md_tcp_port;
    md.sendParam = md_send_params();
    md.maxNumSessions = 64u;

    link->name = name;
    link->own_ip = vos_dottedIP(ip != NULL && *ip != '\0' ? ip : "0.0.0.0");
    if (
        tlc_openSession(
            &link->app,
            link->own_ip,
            0u,
            NULL,
            &pd,
            &md,
            NULL
        ) != TRDP_NO_ERR
    ) {
        return TRDP_INIT_ERR;
    }
    link->active = 1;
    return TRDP_NO_ERR;
}

#ifdef _WIN32
static unsigned __stdcall process_loop(void *unused)
#else
static void *process_loop(void *unused)
#endif
{
    (void)unused;
    while (g_running) {
        int index;
        for (index = 0; index < 2; ++index) {
            if (g_links[index].active) {
                TRDP_FDS_T read_fds;
                TRDP_TIME_T interval;
                TRDP_SOCK_T no_desc = 0;
                INT32 ready;

                mutex_lock(&g_lock);
                FD_ZERO(&read_fds);
                if (
                    tlc_getInterval(
                        g_links[index].app,
                        &interval,
                        &read_fds,
                        &no_desc
                    ) == TRDP_NO_ERR
                ) {
                    if (interval.tv_sec > 0 || interval.tv_usec > 10000) {
                        interval.tv_sec = 0;
                        interval.tv_usec = 10000;
                    }
                    ready = vos_select(no_desc + 1, &read_fds, NULL, NULL, &interval);
                    if (ready < 0) {
                        ready = 0;
                    }
                    (void)tlc_process(g_links[index].app, &read_fds, &ready);
                }
                mutex_unlock(&g_lock);
            }
        }
        sleep_ms(1u);
    }
#ifdef _WIN32
    return 0u;
#else
    return NULL;
#endif
}

static int start_process_thread(void) {
    if (g_process_thread_active) {
        return 1;
    }
#ifdef _WIN32
    {
        uintptr_t handle = _beginthreadex(NULL, 0, process_loop, NULL, 0, NULL);
        if (handle == 0u) {
            return 0;
        }
        g_process_thread = (HANDLE)handle;
    }
#else
    if (pthread_create(&g_process_thread, NULL, process_loop, NULL) != 0) {
        return 0;
    }
#endif
    g_process_thread_active = 1;
    return 1;
}

static void stop_object(object_t *object) {
    int index;
    if (object == NULL || !object->active) {
        return;
    }

    mutex_lock(&g_lock);
    for (index = 0; index < 2; ++index) {
        if (!g_links[index].active || !link_selected(object->link, index)) {
            continue;
        }
        if (object->kind == OBJ_PD_PUBLISHER && object->pub[index] != NULL) {
            (void)tlp_unpublish(g_links[index].app, object->pub[index]);
            object->pub[index] = NULL;
        }
        if (
            (object->kind == OBJ_PD_SUBSCRIBER || object->kind == OBJ_PD_REQUEST)
            && object->sub[index] != NULL
        ) {
            (void)tlp_unsubscribe(g_links[index].app, object->sub[index]);
            object->sub[index] = NULL;
        }
        if (object->kind == OBJ_MD_LISTENER && object->listener[index] != NULL) {
            (void)tlm_delListener(g_links[index].app, object->listener[index]);
            object->listener[index] = NULL;
        }
    }
    mutex_unlock(&g_lock);

    free(object->data);
    memset(object, 0, sizeof(*object));
}

static TRDP_ERR_T start_on_link(
    object_t *object,
    int link_index,
    const char *kind,
    const char *destination,
    const char *source,
    UINT32 cycle_or_timeout,
    const char *transport
) {
    link_t *link = &g_links[link_index];
    TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB;
    TRDP_IP_ADDR_T dest_ip = vos_dottedIP(
        destination != NULL && *destination != '\0' ? destination : "0.0.0.0"
    );
    TRDP_IP_ADDR_T source_ip = vos_dottedIP(
        source != NULL && *source != '\0' ? source : "0.0.0.0"
    );

    if (strcmp(kind, "pd_publisher") == 0) {
        object->kind = OBJ_PD_PUBLISHER;
        if (object->red_id != 0u) {
            TRDP_ERR_T red_error = tlp_setRedundant(
                link->app,
                object->red_id,
                object->red_leader
            );
            if (red_error != TRDP_NO_ERR) {
                return red_error;
            }
        }
        return tlp_publish(
            link->app,
            &object->pub[link_index],
            object,
            pd_callback,
            0u,
            object->com_id,
            object->etb_topo_count,
            object->op_trn_topo_count,
            source_ip,
            dest_ip,
            cycle_or_timeout != 0u ? cycle_or_timeout : 100000u,
            object->red_id,
            flags,
            object->data,
            object->data_len
        );
    }

    if (
        strcmp(kind, "pd_subscriber") == 0
        || strcmp(kind, "pd_request") == 0
    ) {
        TRDP_ERR_T error;
        object->kind = strcmp(kind, "pd_request") == 0
            ? OBJ_PD_REQUEST
            : OBJ_PD_SUBSCRIBER;
        error = tlp_subscribe(
            link->app,
            &object->sub[link_index],
            object,
            pd_callback,
            0u,
            object->com_id,
            object->etb_topo_count,
            object->op_trn_topo_count,
            source_ip,
            0u,
            dest_ip,
            flags,
            cycle_or_timeout != 0u ? cycle_or_timeout : TRDP_DEFAULT_PD_TIMEOUT,
            object->timeout_behavior
        );
        if (error != TRDP_NO_ERR || object->kind != OBJ_PD_REQUEST) {
            return error;
        }

        error = tlp_request(
            link->app,
            object->sub[link_index],
            0u,
            object->com_id,
            object->etb_topo_count,
            object->op_trn_topo_count,
            source_ip,
            dest_ip,
            object->red_id,
            flags,
            object->data,
            object->data_len,
            object->com_id,
            source_ip != 0u ? source_ip : link->own_ip
        );
        if (error != TRDP_NO_ERR) {
            (void)tlp_unsubscribe(link->app, object->sub[link_index]);
            object->sub[link_index] = NULL;
        }
        return error;
    }

    if (strcmp(kind, "md_listener") == 0) {
        object->kind = OBJ_MD_LISTENER;
        object->auto_reply = 1;
        if (transport != NULL && strcmp(transport, "tcp") == 0) {
            flags |= TRDP_FLAGS_TCP;
        }
        return tlm_addListener(
            link->app,
            &object->listener[link_index],
            object,
            md_callback,
            TRUE,
            object->com_id,
            object->etb_topo_count,
            object->op_trn_topo_count,
            source_ip,
            0u,
            dest_ip,
            flags,
            NULL,
            NULL
        );
    }

    return TRDP_PARAM_ERR;
}

static void handle_one_shot_md(
    const char *kind,
    const char *id,
    const char *link_selection,
    const char *destination,
    const char *source,
    const char *transport,
    UINT32 com_id,
    UINT32 etb_topo_count,
    UINT32 op_trn_topo_count,
    const UINT8 *data,
    UINT32 data_len
) {
    int index;
    int sent = 0;
    TRDP_ERR_T error = TRDP_NO_ERR;

    for (index = 0; index < 2; ++index) {
        TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK;
        TRDP_COM_PARAM_T send = md_send_params();
        TRDP_IP_ADDR_T source_ip;
        TRDP_IP_ADDR_T dest_ip;

        if (!g_links[index].active || !link_selected(link_selection, index)) {
            continue;
        }
        if (transport != NULL && strcmp(transport, "tcp") == 0) {
            flags |= TRDP_FLAGS_TCP;
        }
        source_ip = vos_dottedIP(
            source != NULL && *source != '\0' ? source : "0.0.0.0"
        );
        if (source_ip == 0u) {
            source_ip = g_links[index].own_ip;
        }
        dest_ip = vos_dottedIP(
            destination != NULL && *destination != '\0'
                ? destination
                : "0.0.0.0"
        );

        mutex_lock(&g_lock);
        if (strcmp(kind, "md_request") == 0) {
            TRDP_UUID_T session_id;
            error = tlm_request(
                g_links[index].app,
                NULL,
                md_callback,
                &session_id,
                com_id,
                etb_topo_count,
                op_trn_topo_count,
                source_ip,
                dest_ip,
                flags,
                1u,
                5000000u,
                &send,
                data,
                data_len,
                NULL,
                NULL
            );
        } else {
            error = tlm_notify(
                g_links[index].app,
                NULL,
                md_callback,
                com_id,
                etb_topo_count,
                op_trn_topo_count,
                source_ip,
                dest_ip,
                flags,
                &send,
                data,
                data_len,
                NULL,
                NULL
            );
        }
        mutex_unlock(&g_lock);

        if (error != TRDP_NO_ERR) {
            emit_error("TCNOpen MD operation failed");
            return;
        }
        ++sent;
    }

    if (sent == 0) {
        emit_error("selected TRDP link is not active");
        return;
    }
    emit_ack(kind, id);
}

static void handle_object_start(const char *line) {
    char id[64] = {0};
    char kind[32] = {0};
    char name[96] = {0};
    char link_selection[8] = "a";
    char destination[64] = {0};
    char source[64] = {0};
    char payload[MAX_PAYLOAD * 2u + 1u] = {0};
    char transport[16] = "udp";
    char timeout_behavior[16] = "keep";
    char red_state[16] = "leader";
    UINT32 cycle_or_timeout;
    UINT32 com_id;
    UINT32 etb_topo_count;
    UINT32 op_trn_topo_count;
    UINT32 red_id;
    UINT32 data_len = 0u;
    UINT8 *data;
    object_t *object;
    TRDP_ERR_T error = TRDP_NO_ERR;
    int index;
    int started = 0;

    if (
        !json_string(line, "id", id, sizeof(id), NULL)
        || !json_string(line, "kind", kind, sizeof(kind), NULL)
    ) {
        emit_error("object_start requires id and kind");
        return;
    }
    (void)json_string(line, "name", name, sizeof(name), "");
    (void)json_string(line, "link", link_selection, sizeof(link_selection), "a");
    (void)json_string(line, "destination", destination, sizeof(destination), "0.0.0.0");
    (void)json_string(line, "source", source, sizeof(source), "0.0.0.0");
    (void)json_string(line, "payload_hex", payload, sizeof(payload), "");
    (void)json_string(line, "transport", transport, sizeof(transport), "udp");
    (void)json_string(
        line,
        "timeout_behavior",
        timeout_behavior,
        sizeof(timeout_behavior),
        "keep"
    );
    (void)json_string(line, "red_state", red_state, sizeof(red_state), "leader");

    com_id = json_u32(line, "com_id", 0u);
    cycle_or_timeout = json_u32(line, "cycle_us", 100000u);
    etb_topo_count = json_u32(line, "etb_topo_count", 0u);
    op_trn_topo_count = json_u32(line, "op_trn_topo_count", 0u);
    red_id = json_u32(line, "red_id", 0u);

    if (com_id == 0u) {
        emit_error("ComID must be non-zero");
        return;
    }

    data = hex_decode(payload, &data_len);
    if (*payload != '\0' && data == NULL) {
        emit_error("payload_hex is invalid or too large");
        return;
    }

    if (
        strcmp(kind, "md_request") == 0
        || strcmp(kind, "md_notify") == 0
    ) {
        handle_one_shot_md(
            kind,
            id,
            link_selection,
            destination,
            source,
            transport,
            com_id,
            etb_topo_count,
            op_trn_topo_count,
            data,
            data_len
        );
        free(data);
        return;
    }

    if (find_object(id) != NULL) {
        free(data);
        emit_error("object id already active");
        return;
    }

    object = allocate_object(id);
    if (object == NULL) {
        free(data);
        emit_error("too many TRDP objects");
        return;
    }

    (void)snprintf(object->name, sizeof(object->name), "%s", name);
    (void)snprintf(object->link, sizeof(object->link), "%s", link_selection);
    object->com_id = com_id;
    object->etb_topo_count = etb_topo_count;
    object->op_trn_topo_count = op_trn_topo_count;
    object->red_id = red_id;
    object->red_leader = strcmp(red_state, "follower") != 0 ? TRUE : FALSE;
    object->timeout_behavior = strcmp(timeout_behavior, "zero") == 0
        ? TRDP_TO_SET_TO_ZERO
        : TRDP_TO_KEEP_LAST_VALUE;
    object->data = data;
    object->data_len = data_len;

    mutex_lock(&g_lock);
    for (index = 0; index < 2; ++index) {
        if (!g_links[index].active || !link_selected(link_selection, index)) {
            continue;
        }
        error = start_on_link(
            object,
            index,
            kind,
            destination,
            source,
            cycle_or_timeout,
            transport
        );
        if (error != TRDP_NO_ERR) {
            break;
        }
        ++started;
    }
    mutex_unlock(&g_lock);

    if (error != TRDP_NO_ERR || started == 0) {
        stop_object(object);
        emit_error("TCNOpen object_start failed");
        return;
    }
    emit_ack("object_start", id);
}

static void handle_object_update(const char *line) {
    char id[64] = {0};
    char payload[MAX_PAYLOAD * 2u + 1u] = {0};
    UINT32 data_len = 0u;
    UINT8 *data;
    object_t *object;
    TRDP_ERR_T error = TRDP_NO_ERR;
    int index;

    if (
        !json_string(line, "id", id, sizeof(id), NULL)
        || !json_string(line, "payload_hex", payload, sizeof(payload), "")
    ) {
        emit_error("object_update requires id");
        return;
    }

    object = find_object(id);
    if (object == NULL) {
        emit_error("TRDP object is not active");
        return;
    }
    data = hex_decode(payload, &data_len);
    if (*payload != '\0' && data == NULL) {
        emit_error("payload_hex is invalid or too large");
        return;
    }

    mutex_lock(&g_lock);
    if (object->kind == OBJ_PD_PUBLISHER) {
        for (index = 0; index < 2; ++index) {
            if (
                g_links[index].active
                && object->pub[index] != NULL
                && link_selected(object->link, index)
            ) {
                error = tlp_put(
                    g_links[index].app,
                    object->pub[index],
                    data,
                    data_len
                );
                if (error != TRDP_NO_ERR) {
                    break;
                }
            }
        }
    }
    if (error == TRDP_NO_ERR) {
        free(object->data);
        object->data = data;
        object->data_len = data_len;
        data = NULL;
    }
    mutex_unlock(&g_lock);
    free(data);

    if (error != TRDP_NO_ERR) {
        emit_error("TCNOpen object_update failed");
        return;
    }
    emit_ack("object_update", id);
}

/* ---------- dynamic libpcap / Npcap live capture ---------- */

typedef struct pcap pcap_t;
typedef unsigned int bpf_u_int32;

struct tau_pcap_pkthdr {
    struct timeval ts;
    bpf_u_int32 caplen;
    bpf_u_int32 len;
};

struct bpf_insn_tau {
    unsigned short code;
    unsigned char jt;
    unsigned char jf;
    bpf_u_int32 k;
};

struct bpf_program_tau {
    unsigned int bf_len;
    struct bpf_insn_tau *bf_insns;
};

typedef pcap_t *(*fn_pcap_open_live)(const char *, int, int, int, char *);
typedef int (*fn_pcap_next_ex)(
    pcap_t *,
    struct tau_pcap_pkthdr **,
    const unsigned char **
);
typedef void (*fn_pcap_close)(pcap_t *);
typedef int (*fn_pcap_compile)(
    pcap_t *,
    struct bpf_program_tau *,
    const char *,
    int,
    bpf_u_int32
);
typedef int (*fn_pcap_setfilter)(pcap_t *, struct bpf_program_tau *);
typedef void (*fn_pcap_freecode)(struct bpf_program_tau *);
typedef int (*fn_pcap_datalink)(pcap_t *);
typedef void (*fn_pcap_breakloop)(pcap_t *);
typedef const char *(*fn_pcap_geterr)(pcap_t *);

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
static pcap_t *g_pcap;
static int g_capture_linktype = LINKTYPE_ETHERNET;
static volatile int g_capture_running = 0;
static bridge_thread_t g_capture_thread;
static int g_capture_thread_active = 0;

static void *dynamic_symbol(const char *name) {
#ifdef _WIN32
    return (void *)GetProcAddress((HMODULE)g_pcap_library, name);
#else
    return dlsym(g_pcap_library, name);
#endif
}

static void unload_pcap_library(void) {
    if (g_pcap_library == NULL) {
        return;
    }
#ifdef _WIN32
    (void)FreeLibrary((HMODULE)g_pcap_library);
#else
    (void)dlclose(g_pcap_library);
#endif
    g_pcap_library = NULL;
}

static int load_pcap(void) {
    if (g_pcap_library != NULL) {
        return 1;
    }

#ifdef _WIN32
    {
        char system_directory[MAX_PATH];
        char npcap_path[MAX_PATH + 32];
        UINT length = GetSystemDirectoryA(system_directory, MAX_PATH);
        if (length > 0u && length < MAX_PATH) {
            (void)snprintf(
                npcap_path,
                sizeof(npcap_path),
                "%s\\Npcap\\wpcap.dll",
                system_directory
            );
            g_pcap_library = (void *)LoadLibraryA(npcap_path);
        }
        if (g_pcap_library == NULL) {
            g_pcap_library = (void *)LoadLibraryA("wpcap.dll");
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

    if (
        dyn_open_live == NULL
        || dyn_next_ex == NULL
        || dyn_close == NULL
        || dyn_compile == NULL
        || dyn_setfilter == NULL
        || dyn_freecode == NULL
        || dyn_datalink == NULL
    ) {
        unload_pcap_library();
        return 0;
    }
    return 1;
}

static uint16_t read_be16(const unsigned char *data) {
    return (uint16_t)(((uint16_t)data[0] << 8) | data[1]);
}

static uint32_t read_be32(const unsigned char *data) {
    return ((uint32_t)data[0] << 24)
        | ((uint32_t)data[1] << 16)
        | ((uint32_t)data[2] << 8)
        | (uint32_t)data[3];
}

static int capture_network_offset(
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
        while (
            ether_type == 0x8100u
            || ether_type == 0x88a8u
            || ether_type == 0x9100u
        ) {
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
        if (length < 16u) {
            return 0;
        }
        *offset = 16u;
        return 1;
    }
    if (linktype == LINKTYPE_LINUX_SLL2) {
        if (length < 20u) {
            return 0;
        }
        *offset = 20u;
        return 1;
    }
    if (linktype == LINKTYPE_NULL) {
        if (length < 4u) {
            return 0;
        }
        *offset = 4u;
        return 1;
    }
    if (linktype == LINKTYPE_RAW) {
        *offset = 0u;
        return 1;
    }
    return 0;
}

static int valid_message_type(const char *message_type) {
    static const char *known[] = {
        "Pd", "Pp", "Pr", "Pe", "Mn", "Mr", "Mp", "Mq", "Mc", "Me"
    };
    size_t index;
    for (index = 0u; index < sizeof(known) / sizeof(known[0]); ++index) {
        if (strcmp(message_type, known[index]) == 0) {
            return 1;
        }
    }
    return 0;
}

static void capture_emit(
    const struct tau_pcap_pkthdr *header,
    const unsigned char *frame
) {
    size_t ip_offset;
    size_t ip_header_length;
    size_t transport_offset;
    size_t trdp_offset;
    size_t transport_header_length;
    size_t trdp_header_length;
    size_t data_start;
    size_t available_data;
    uint16_t fragment;
    uint16_t source_port;
    uint16_t destination_port;
    unsigned char protocol;
    uint32_t data_length;
    char message_type[3];

    if (
        header == NULL
        || frame == NULL
        || !capture_network_offset(
            frame,
            (size_t)header->caplen,
            g_capture_linktype,
            &ip_offset
        )
        || (size_t)header->caplen < ip_offset + 20u
        || (frame[ip_offset] >> 4) != 4u
    ) {
        return;
    }

    ip_header_length = (size_t)(frame[ip_offset] & 0x0fu) * 4u;
    if (
        ip_header_length < 20u
        || (size_t)header->caplen < ip_offset + ip_header_length
    ) {
        return;
    }
    fragment = read_be16(frame + ip_offset + 6u);
    if ((fragment & 0x1fffu) != 0u) {
        return;
    }

    protocol = frame[ip_offset + 9u];
    transport_offset = ip_offset + ip_header_length;
    if (protocol == 17u) {
        if ((size_t)header->caplen < transport_offset + 8u) {
            return;
        }
        source_port = read_be16(frame + transport_offset);
        destination_port = read_be16(frame + transport_offset + 2u);
        transport_header_length = 8u;
    } else if (protocol == 6u) {
        if ((size_t)header->caplen < transport_offset + 20u) {
            return;
        }
        transport_header_length =
            (size_t)(frame[transport_offset + 12u] >> 4) * 4u;
        if (
            transport_header_length < 20u
            || (size_t)header->caplen < transport_offset + transport_header_length
        ) {
            return;
        }
        source_port = read_be16(frame + transport_offset);
        destination_port = read_be16(frame + transport_offset + 2u);
    } else {
        return;
    }

    trdp_offset = transport_offset + transport_header_length;
    if ((size_t)header->caplen < trdp_offset + 24u) {
        return;
    }
    message_type[0] = (char)frame[trdp_offset + 6u];
    message_type[1] = (char)frame[trdp_offset + 7u];
    message_type[2] = '\0';
    if (!valid_message_type(message_type)) {
        return;
    }

    data_length = read_be32(frame + trdp_offset + 20u);
    trdp_header_length = message_type[0] == 'M' ? 116u : 40u;
    data_start = trdp_offset + trdp_header_length;
    available_data = (size_t)header->caplen > data_start
        ? (size_t)header->caplen - data_start
        : 0u;
    if (available_data > (size_t)data_length) {
        available_data = (size_t)data_length;
    }

    mutex_lock(&g_out_lock);
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"capture\",\"link\":\"capture\","
        "\"link_type\":%d,\"timestamp_us\":%llu,\"transport\":\"%s\","
        "\"src_port\":%u,\"dest_port\":%u,\"msg_type\":\"%s\","
        "\"com_id\":%u,\"seq_count\":%u,\"protocol_version\":%u,"
        "\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,\"src_ip\":\""
        "%u.%u.%u.%u\",\"dest_ip\":\"%u.%u.%u.%u\",\"data_len\":%u,"
        "\"payload_hex\":\"",
        g_capture_linktype,
        (unsigned long long)header->ts.tv_sec * 1000000ull
            + (unsigned long long)header->ts.tv_usec,
        protocol == 17u ? "udp" : "tcp",
        (unsigned int)source_port,
        (unsigned int)destination_port,
        message_type,
        (unsigned int)read_be32(frame + trdp_offset + 8u),
        (unsigned int)read_be32(frame + trdp_offset),
        (unsigned int)read_be16(frame + trdp_offset + 4u),
        (unsigned int)read_be32(frame + trdp_offset + 12u),
        (unsigned int)read_be32(frame + trdp_offset + 16u),
        frame[ip_offset + 12u],
        frame[ip_offset + 13u],
        frame[ip_offset + 14u],
        frame[ip_offset + 15u],
        frame[ip_offset + 16u],
        frame[ip_offset + 17u],
        frame[ip_offset + 18u],
        frame[ip_offset + 19u],
        (unsigned int)data_length
    );
    print_hex(stdout, frame + data_start, (UINT32)available_data);
    fputs("\",\"raw_frame_hex\":\"", stdout);
    print_hex(stdout, frame, (UINT32)header->caplen);
    fputs("\"}\n", stdout);
    fflush(stdout);
    mutex_unlock(&g_out_lock);
}

#ifdef _WIN32
static unsigned __stdcall capture_loop(void *unused)
#else
static void *capture_loop(void *unused)
#endif
{
    (void)unused;
    while (g_capture_running && g_pcap != NULL) {
        struct tau_pcap_pkthdr *header = NULL;
        const unsigned char *data = NULL;
        int result = dyn_next_ex(g_pcap, &header, &data);
        if (result == 1) {
            capture_emit(header, data);
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

static void capture_stop(void) {
    if (!g_capture_thread_active && g_pcap == NULL) {
        return;
    }
    g_capture_running = 0;
    if (g_pcap != NULL && dyn_breakloop != NULL) {
        dyn_breakloop(g_pcap);
    }
    if (g_capture_thread_active) {
        thread_join(g_capture_thread);
        g_capture_thread_active = 0;
    }
    if (g_pcap != NULL) {
        dyn_close(g_pcap);
        g_pcap = NULL;
    }
}

static void capture_start(const char *line) {
    char interface_name[512] = {0};
    char filter[1024] =
        "udp port 17224 or udp port 17225 or tcp port 17225";
    char error_buffer[256] = {0};
    struct bpf_program_tau program;

    if (
        !json_string(
            line,
            "interface",
            interface_name,
            sizeof(interface_name),
            NULL
        )
        || *interface_name == '\0'
    ) {
        emit_error("live capture requires interface name");
        return;
    }
    (void)json_string(line, "filter", filter, sizeof(filter), filter);

    if (!load_pcap()) {
        emit_error(
            "libpcap/Npcap not found. Windows users must install Npcap separately."
        );
        return;
    }

    capture_stop();
    g_pcap = dyn_open_live(interface_name, 65535, 1, 100, error_buffer);
    if (g_pcap == NULL) {
        emit_error(
            *error_buffer != '\0' ? error_buffer : "pcap_open_live failed"
        );
        return;
    }

    g_capture_linktype = dyn_datalink(g_pcap);
    if (
        g_capture_linktype != LINKTYPE_ETHERNET
        && g_capture_linktype != LINKTYPE_LINUX_SLL
        && g_capture_linktype != LINKTYPE_LINUX_SLL2
        && g_capture_linktype != LINKTYPE_NULL
        && g_capture_linktype != LINKTYPE_RAW
    ) {
        capture_stop();
        emit_error("capture interface link type is not supported");
        return;
    }

    memset(&program, 0, sizeof(program));
    if (dyn_compile(g_pcap, &program, filter, 1, 0xffffffffu) != 0) {
        const char *pcap_error = dyn_geterr != NULL ? dyn_geterr(g_pcap) : NULL;
        char message[384];
        (void)snprintf(
            message,
            sizeof(message),
            "pcap filter compile failed: %s",
            pcap_error != NULL ? pcap_error : "unknown error"
        );
        capture_stop();
        emit_error(message);
        return;
    }
    if (dyn_setfilter(g_pcap, &program) != 0) {
        const char *pcap_error = dyn_geterr != NULL ? dyn_geterr(g_pcap) : NULL;
        char message[384];
        dyn_freecode(&program);
        (void)snprintf(
            message,
            sizeof(message),
            "pcap_setfilter failed: %s",
            pcap_error != NULL ? pcap_error : "unknown error"
        );
        capture_stop();
        emit_error(message);
        return;
    }
    dyn_freecode(&program);

    g_capture_running = 1;
#ifdef _WIN32
    {
        uintptr_t handle = _beginthreadex(NULL, 0, capture_loop, NULL, 0, NULL);
        if (handle == 0u) {
            g_capture_running = 0;
            capture_stop();
            emit_error("capture thread failed");
            return;
        }
        g_capture_thread = (HANDLE)handle;
    }
#else
    if (pthread_create(&g_capture_thread, NULL, capture_loop, NULL) != 0) {
        g_capture_running = 0;
        capture_stop();
        emit_error("capture thread failed");
        return;
    }
#endif
    g_capture_thread_active = 1;
    emit_ack("capture_start", NULL);
}

/* ---------- process commands ---------- */

static void close_links(void) {
    int index;
    mutex_lock(&g_lock);
    for (index = 0; index < 2; ++index) {
        if (g_links[index].active) {
            (void)tlc_closeSession(g_links[index].app);
            memset(&g_links[index], 0, sizeof(g_links[index]));
        }
    }
    mutex_unlock(&g_lock);
}

static void bridge_shutdown(void) {
    int index;
    capture_stop();

    for (index = 0; index < MAX_OBJECTS; ++index) {
        if (g_objects[index].active) {
            stop_object(&g_objects[index]);
        }
    }

    g_running = 0;
    if (g_process_thread_active) {
        thread_join(g_process_thread);
        g_process_thread_active = 0;
    }

    close_links();
    if (g_tlc_initialized) {
        (void)tlc_terminate();
        g_tlc_initialized = 0;
    }
}

static void handle_open(const char *line) {
    char link_a_ip[64] = "0.0.0.0";
    char link_b_ip[64] = "0.0.0.0";
    int link_b_enabled = json_bool(line, "link_b_enabled", 0);
    UINT16 pd_port = (UINT16)json_u32(line, "pd_port", DEFAULT_PD_PORT);
    UINT16 md_udp_port =
        (UINT16)json_u32(line, "md_udp_port", DEFAULT_MD_PORT);
    UINT16 md_tcp_port =
        (UINT16)json_u32(line, "md_tcp_port", DEFAULT_MD_PORT);
    TRDP_ERR_T error;

    if (g_tlc_initialized) {
        emit_error("TRDP Node is already open");
        return;
    }
    (void)json_string(
        line,
        "link_a_ip",
        link_a_ip,
        sizeof(link_a_ip),
        "0.0.0.0"
    );
    (void)json_string(
        line,
        "link_b_ip",
        link_b_ip,
        sizeof(link_b_ip),
        "0.0.0.0"
    );

    if (tlc_init(NULL, NULL, NULL) != TRDP_NO_ERR) {
        emit_error("TCNOpen tlc_init failed");
        return;
    }
    g_tlc_initialized = 1;

    mutex_lock(&g_lock);
    error = open_link(
        &g_links[0],
        'A',
        link_a_ip,
        pd_port,
        md_udp_port,
        md_tcp_port
    );
    if (error == TRDP_NO_ERR && link_b_enabled) {
        error = open_link(
            &g_links[1],
            'B',
            link_b_ip,
            pd_port,
            md_udp_port,
            md_tcp_port
        );
    }
    mutex_unlock(&g_lock);

    if (error != TRDP_NO_ERR) {
        close_links();
        (void)tlc_terminate();
        g_tlc_initialized = 0;
        emit_error(
            "TCNOpen tlc_openSession failed; verify local interface IPv4 and ports"
        );
        return;
    }
    if (!start_process_thread()) {
        close_links();
        (void)tlc_terminate();
        g_tlc_initialized = 0;
        emit_error("TRDP process thread failed");
        return;
    }
    emit_ack("open", NULL);
}

static void handle_monitor_open(void) {
    emit_ack("monitor_open", NULL);
}

int main(void) {
    char *line = (char *)malloc(MAX_LINE);
    if (line == NULL) {
        return 2;
    }

    mutex_init(&g_lock);
    mutex_init(&g_out_lock);
    memset(g_links, 0, sizeof(g_links));
    memset(g_objects, 0, sizeof(g_objects));

    while (g_running && fgets(line, MAX_LINE, stdin) != NULL) {
        char command[64] = {0};
        char id[64] = {0};

        if (!json_string(line, "command", command, sizeof(command), NULL)) {
            emit_error("missing command");
            continue;
        }

        if (strcmp(command, "open") == 0) {
            handle_open(line);
        } else if (strcmp(command, "monitor_open") == 0) {
            handle_monitor_open();
        } else if (strcmp(command, "object_start") == 0) {
            handle_object_start(line);
        } else if (strcmp(command, "object_update") == 0) {
            handle_object_update(line);
        } else if (strcmp(command, "object_stop") == 0) {
            if (!json_string(line, "id", id, sizeof(id), NULL)) {
                emit_error("object_stop requires id");
            } else {
                object_t *object = find_object(id);
                if (object != NULL) {
                    stop_object(object);
                }
                emit_ack("object_stop", id);
            }
        } else if (strcmp(command, "capture_start") == 0) {
            capture_start(line);
        } else if (strcmp(command, "capture_stop") == 0) {
            capture_stop();
            emit_ack("capture_stop", NULL);
        } else if (strcmp(command, "shutdown") == 0) {
            emit_ack("shutdown", NULL);
            break;
        } else {
            emit_error("unknown TRDP bridge command");
        }
    }

    bridge_shutdown();
    free(line);
    return 0;
}
