/*
 * TauTerm TRDP bridge
 *
 * TauTerm-owned adapter code (MIT OR Apache-2.0). It links against TCNOpen,
 * which remains MPL-2.0 licensed and is obtained separately by bootstrap scripts.
 * No TCNOpen source is copied into this file.
 */
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "trdp_if_light.h"
#include "vos_sock.h"
#include "vos_thread.h"

#define TAU_MAX_OBJECTS 128
#define TAU_MAX_PAYLOAD 65536
#define TAU_ID_LEN 64
#define TAU_LINE_LEN 131072

#ifndef TRDP_FLAGS_CALLBACK
#define TRDP_FLAGS_CALLBACK 0x04u
#endif
#ifndef TRDP_FLAGS_TCP
#define TRDP_FLAGS_TCP 0x08u
#endif

typedef enum {
    TAU_NONE = 0,
    TAU_PD_PUBLISHER,
    TAU_PD_SUBSCRIBER,
    TAU_PD_REQUEST,
    TAU_MD_REQUEST,
    TAU_MD_LISTENER,
    TAU_MD_NOTIFY
} TAU_KIND;

typedef struct {
    char id[TAU_ID_LEN];
    TAU_KIND kind;
    uint32_t com_id;
    uint32_t etb_topo;
    uint32_t op_topo;
    uint32_t red_id;
    uint32_t timeout_us;
    uint32_t cycle_us;
    int use_a;
    int use_b;
    int tcp;
    char destination[64];
    char source[64];
    uint8_t payload[TAU_MAX_PAYLOAD];
    uint32_t payload_len;
    TRDP_PUB_T pub_a;
    TRDP_PUB_T pub_b;
    TRDP_SUB_T sub_a;
    TRDP_SUB_T sub_b;
    TRDP_LIS_T lis_a;
    TRDP_LIS_T lis_b;
} TAU_OBJECT;

typedef struct {
    TRDP_APP_SESSION_T app_a;
    TRDP_APP_SESSION_T app_b;
    char ip_a[64];
    char ip_b[64];
    int have_b;
    volatile int running;
    VOS_THREAD_T process_thread;
    VOS_MUTEX_T object_mutex;
    VOS_MUTEX_T output_mutex;
    TAU_OBJECT objects[TAU_MAX_OBJECTS];
} TAU_STATE;

static TAU_STATE g_state;

static void output_lock(void) {
    if (g_state.output_mutex != NULL) {
        (void)vos_mutexLock(g_state.output_mutex);
    }
}

static void output_unlock(void) {
    if (g_state.output_mutex != NULL) {
        (void)vos_mutexUnlock(g_state.output_mutex);
    }
}

static void json_escape(FILE *out, const char *value) {
    const unsigned char *p = (const unsigned char *)value;
    fputc('"', out);
    while (*p != 0u) {
        switch (*p) {
            case '\\': fputs("\\\\", out); break;
            case '"': fputs("\\\"", out); break;
            case '\n': fputs("\\n", out); break;
            case '\r': fputs("\\r", out); break;
            case '\t': fputs("\\t", out); break;
            default:
                if (*p < 0x20u) {
                    fprintf(out, "\\u%04x", (unsigned int)*p);
                } else {
                    fputc((int)*p, out);
                }
                break;
        }
        ++p;
    }
    fputc('"', out);
}

static void emit_error(const char *message) {
    output_lock();
    fputs("{\"event\":\"error\",\"error\":", stdout);
    json_escape(stdout, message);
    fputs("}\n", stdout);
    fflush(stdout);
    output_unlock();
}

static void emit_ok(const char *operation, const char *id) {
    output_lock();
    fputs("{\"event\":\"operation\",\"operation\":", stdout);
    json_escape(stdout, operation);
    if (id != NULL) {
        fputs(",\"id\":", stdout);
        json_escape(stdout, id);
    }
    fputs(",\"ok\":true}\n", stdout);
    fflush(stdout);
    output_unlock();
}

static void emit_hex(const uint8_t *data, uint32_t size) {
    static const char digits[] = "0123456789ABCDEF";
    uint32_t i;
    fputc('"', stdout);
    for (i = 0u; i < size; ++i) {
        fputc(digits[(data[i] >> 4) & 0x0fu], stdout);
        fputc(digits[data[i] & 0x0fu], stdout);
    }
    fputc('"', stdout);
}

static const char *link_name(TRDP_APP_SESSION_T app) {
    return app == g_state.app_b ? "b" : "a";
}

static void emit_pd(TRDP_APP_SESSION_T app, const TRDP_PD_INFO_T *info, const uint8_t *data, uint32_t size) {
    output_lock();
    fprintf(stdout,
            "{\"event\":\"packet\",\"kind\":\"pd\",\"link\":\"%s\",\"com_id\":%u,"
            "\"seq_count\":%u,\"protocol_version\":%u,\"etb_topo_count\":%u,"
            "\"op_trn_topo_count\":%u,\"data_len\":%u,\"result_code\":%d,"
            "\"src_ip_u32\":%u,\"dest_ip_u32\":%u,\"msg_type_u16\":%u,\"payload_hex\":",
            link_name(app), (unsigned int)info->comId, (unsigned int)info->seqCount,
            (unsigned int)info->protVersion, (unsigned int)info->etbTopoCnt,
            (unsigned int)info->opTrnTopoCnt, (unsigned int)size, (int)info->resultCode,
            (unsigned int)info->srcIpAddr, (unsigned int)info->destIpAddr,
            (unsigned int)info->msgType);
    emit_hex(data, size);
    fputs("}\n", stdout);
    fflush(stdout);
    output_unlock();
}

static void emit_md(TRDP_APP_SESSION_T app, const TRDP_MD_INFO_T *info, const uint8_t *data, uint32_t size) {
    output_lock();
    fprintf(stdout,
            "{\"event\":\"packet\",\"kind\":\"md\",\"link\":\"%s\",\"com_id\":%u,"
            "\"seq_count\":%u,\"protocol_version\":%u,\"etb_topo_count\":%u,"
            "\"op_trn_topo_count\":%u,\"data_len\":%u,\"result_code\":%d,"
            "\"reply_status\":%d,\"user_status\":%u,\"num_replies\":%u,"
            "\"src_ip_u32\":%u,\"dest_ip_u32\":%u,\"msg_type_u16\":%u,\"payload_hex\":",
            link_name(app), (unsigned int)info->comId, (unsigned int)info->seqCount,
            (unsigned int)info->protVersion, (unsigned int)info->etbTopoCnt,
            (unsigned int)info->opTrnTopoCnt, (unsigned int)size, (int)info->resultCode,
            (int)info->replyStatus, (unsigned int)info->userStatus,
            (unsigned int)info->numReplies, (unsigned int)info->srcIpAddr,
            (unsigned int)info->destIpAddr, (unsigned int)info->msgType);
    emit_hex(data, size);
    fputs("}\n", stdout);
    fflush(stdout);
    output_unlock();
}

static void pd_callback(void *ref, TRDP_APP_SESSION_T app, const TRDP_PD_INFO_T *info,
                        uint8_t *data, uint32_t size) {
    (void)ref;
    if (info != NULL) {
        emit_pd(app, info, data != NULL ? data : (uint8_t *)"", data != NULL ? size : 0u);
    }
}

static void md_callback(void *ref, TRDP_APP_SESSION_T app, const TRDP_MD_INFO_T *info,
                        uint8_t *data, uint32_t size) {
    TAU_OBJECT *object = (TAU_OBJECT *)ref;
    if (info == NULL) {
        return;
    }
    emit_md(app, info, data != NULL ? data : (uint8_t *)"", data != NULL ? size : 0u);
    /* A listener doubles as a simple replier. This is deliberately explicit:
       requesters and notifies never auto-reply. */
    if (object != NULL && object->kind == TAU_MD_LISTENER && info->resultCode == TRDP_NO_ERR) {
        TRDP_SEND_PARAM_T send_param = TRDP_MD_DEFAULT_SEND_PARAM;
        if (info->msgType == TRDP_MSG_MR) {
            (void)tlm_reply(app, &info->sessionId, object->com_id, 0u, &send_param,
                            object->payload, object->payload_len);
        }
    }
}

static const char *find_key(const char *json, const char *key) {
    static char pattern[128];
    const char *p;
    (void)snprintf(pattern, sizeof(pattern), "\"%s\"", key);
    p = strstr(json, pattern);
    if (p == NULL) return NULL;
    p += strlen(pattern);
    while (*p != 0 && isspace((unsigned char)*p)) ++p;
    if (*p != ':') return NULL;
    ++p;
    while (*p != 0 && isspace((unsigned char)*p)) ++p;
    return p;
}

static int json_string(const char *json, const char *key, char *out, size_t cap, const char *fallback) {
    const char *p = find_key(json, key);
    size_t n = 0u;
    if (p == NULL || *p != '"') {
        if (fallback != NULL) {
            (void)snprintf(out, cap, "%s", fallback);
            return 1;
        }
        return 0;
    }
    ++p;
    while (*p != 0 && *p != '"' && n + 1u < cap) {
        if (*p == '\\' && p[1] != 0) {
            ++p;
            switch (*p) {
                case 'n': out[n++] = '\n'; break;
                case 'r': out[n++] = '\r'; break;
                case 't': out[n++] = '\t'; break;
                default: out[n++] = *p; break;
            }
        } else {
            out[n++] = *p;
        }
        ++p;
    }
    out[n] = 0;
    return 1;
}

static uint32_t json_u32(const char *json, const char *key, uint32_t fallback) {
    const char *p = find_key(json, key);
    char *end = NULL;
    unsigned long value;
    if (p == NULL) return fallback;
    value = strtoul(p, &end, 0);
    return end == p ? fallback : (uint32_t)value;
}

static int json_bool(const char *json, const char *key, int fallback) {
    const char *p = find_key(json, key);
    if (p == NULL) return fallback;
    if (strncmp(p, "true", 4u) == 0) return 1;
    if (strncmp(p, "false", 5u) == 0) return 0;
    return fallback;
}

static int hex_value(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static uint32_t json_hex(const char *json, const char *key, uint8_t *out, uint32_t cap) {
    char text[TAU_MAX_PAYLOAD * 2u + 1u];
    uint32_t length = 0u;
    size_t i;
    if (!json_string(json, key, text, sizeof(text), "")) return 0u;
    for (i = 0u; text[i] != 0 && text[i + 1u] != 0 && length < cap; i += 2u) {
        int hi = hex_value(text[i]);
        int lo = hex_value(text[i + 1u]);
        if (hi < 0 || lo < 0) break;
        out[length++] = (uint8_t)((hi << 4) | lo);
    }
    return length;
}

static TAU_KIND parse_kind(const char *value) {
    if (strcmp(value, "pd_publisher") == 0) return TAU_PD_PUBLISHER;
    if (strcmp(value, "pd_subscriber") == 0) return TAU_PD_SUBSCRIBER;
    if (strcmp(value, "pd_request") == 0) return TAU_PD_REQUEST;
    if (strcmp(value, "md_request") == 0) return TAU_MD_REQUEST;
    if (strcmp(value, "md_listener") == 0) return TAU_MD_LISTENER;
    if (strcmp(value, "md_notify") == 0) return TAU_MD_NOTIFY;
    return TAU_NONE;
}

static TAU_OBJECT *find_object(const char *id, int create) {
    size_t i;
    TAU_OBJECT *free_slot = NULL;
    for (i = 0u; i < TAU_MAX_OBJECTS; ++i) {
        if (g_state.objects[i].id[0] == 0 && free_slot == NULL) free_slot = &g_state.objects[i];
        if (strcmp(g_state.objects[i].id, id) == 0) return &g_state.objects[i];
    }
    if (create && free_slot != NULL) {
        memset(free_slot, 0, sizeof(*free_slot));
        (void)snprintf(free_slot->id, sizeof(free_slot->id), "%s", id);
        return free_slot;
    }
    return NULL;
}

static TRDP_FLAGS_T flags_for(const TAU_OBJECT *object) {
    TRDP_FLAGS_T flags = (TRDP_FLAGS_T)(TRDP_FLAGS_CALLBACK);
    if (object->tcp) flags = (TRDP_FLAGS_T)(flags | TRDP_FLAGS_TCP);
    return flags;
}

static TRDP_ERR_T start_on_session(TAU_OBJECT *object, TRDP_APP_SESSION_T app, int link_b) {
    TRDP_IP_ADDR_T src = vos_dottedIP(object->source);
    TRDP_IP_ADDR_T dst = vos_dottedIP(object->destination);
    TRDP_SEND_PARAM_T send_param = object->kind == TAU_PD_PUBLISHER
        ? (TRDP_SEND_PARAM_T)TRDP_PD_DEFAULT_SEND_PARAM
        : (TRDP_SEND_PARAM_T)TRDP_MD_DEFAULT_SEND_PARAM;
    TRDP_FLAGS_T flags = flags_for(object);
    TRDP_ERR_T result = TRDP_NO_ERR;

    if (object->kind == TAU_PD_PUBLISHER) {
        TRDP_PUB_T *handle = link_b ? &object->pub_b : &object->pub_a;
        result = tlp_publish(app, handle, object, NULL, 0u, object->com_id,
                             object->etb_topo, object->op_topo, src, dst,
                             object->cycle_us, object->red_id, TRDP_FLAGS_NONE,
                             &send_param, object->payload, object->payload_len);
    } else if (object->kind == TAU_PD_SUBSCRIBER || object->kind == TAU_PD_REQUEST) {
        TRDP_SUB_T *handle = link_b ? &object->sub_b : &object->sub_a;
        result = tlp_subscribe(app, handle, object, pd_callback, 0u, object->com_id,
                               object->etb_topo, object->op_topo, 0u, 0u, dst,
                               (TRDP_FLAGS_T)(TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB),
                               NULL, object->timeout_us, TRDP_TO_KEEP_LAST_VALUE);
        if (result == TRDP_NO_ERR && object->kind == TAU_PD_REQUEST) {
            result = tlp_request(app, *handle, 0u, object->com_id, object->etb_topo,
                                 object->op_topo, src, dst, object->red_id,
                                 TRDP_FLAGS_NONE, &send_param, object->payload,
                                 object->payload_len, object->com_id, src);
        }
    } else if (object->kind == TAU_MD_LISTENER) {
        TRDP_LIS_T *handle = link_b ? &object->lis_b : &object->lis_a;
        result = tlm_addListener(app, handle, object, md_callback, TRUE,
                                 object->com_id, object->etb_topo, object->op_topo,
                                 0u, 0u, dst, flags, NULL, NULL);
    } else if (object->kind == TAU_MD_REQUEST) {
        TRDP_UUID_T session_id;
        memset(&session_id, 0, sizeof(session_id));
        result = tlm_request(app, object, md_callback, &session_id, object->com_id,
                             object->etb_topo, object->op_topo, src, dst, flags,
                             1u, object->timeout_us, &send_param, object->payload,
                             object->payload_len, NULL, NULL);
    } else if (object->kind == TAU_MD_NOTIFY) {
        result = tlm_notify(app, object, md_callback, object->com_id,
                            object->etb_topo, object->op_topo, src, dst, flags,
                            &send_param, object->payload, object->payload_len,
                            NULL, NULL);
    } else {
        result = TRDP_PARAM_ERR;
    }
    return result;
}

static void stop_on_session(TAU_OBJECT *object, TRDP_APP_SESSION_T app, int link_b) {
    TRDP_PUB_T pub = link_b ? object->pub_b : object->pub_a;
    TRDP_SUB_T sub = link_b ? object->sub_b : object->sub_a;
    TRDP_LIS_T lis = link_b ? object->lis_b : object->lis_a;
    if (object->kind == TAU_PD_PUBLISHER && pub != NULL) (void)tlp_unpublish(app, pub);
    if ((object->kind == TAU_PD_SUBSCRIBER || object->kind == TAU_PD_REQUEST) && sub != NULL) (void)tlp_unsubscribe(app, sub);
    if (object->kind == TAU_MD_LISTENER && lis != NULL) (void)tlm_delListener(app, lis);
    if (link_b) {
        object->pub_b = NULL; object->sub_b = NULL; object->lis_b = NULL;
    } else {
        object->pub_a = NULL; object->sub_a = NULL; object->lis_a = NULL;
    }
}

static void process_loop(void *argument) {
    (void)argument;
    while (g_state.running) {
        TRDP_APP_SESSION_T sessions[2] = { g_state.app_a, g_state.app_b };
        int i;
        for (i = 0; i < 2; ++i) {
            if (sessions[i] != NULL) {
                TRDP_FDS_T rfds;
                TRDP_TIME_T interval;
                INT32 no_desc = 0;
                INT32 count = 0;
                FD_ZERO(&rfds);
                if (tlc_getInterval(sessions[i], &interval, &rfds, &no_desc) == TRDP_NO_ERR) {
                    /* Keep the bridge responsive when the stack returns a long interval. */
                    if (interval.tv_sec > 0 || interval.tv_usec > 10000) {
                        interval.tv_sec = 0;
                        interval.tv_usec = 10000;
                    }
                    count = vos_select(no_desc + 1, &rfds, NULL, NULL, &interval);
                    if (count >= 0) (void)tlc_process(sessions[i], &rfds, &count);
                }
            }
        }
        if (g_state.app_a == NULL && g_state.app_b == NULL) (void)vos_threadDelay(10000u);
    }
}

static int open_session(const char *json) {
    TRDP_ERR_T result;
    if (g_state.app_a != NULL || g_state.app_b != NULL) return 1;
    (void)json_string(json, "link_a_ip", g_state.ip_a, sizeof(g_state.ip_a), "0.0.0.0");
    g_state.have_b = json_bool(json, "link_b_enabled", 0);
    (void)json_string(json, "link_b_ip", g_state.ip_b, sizeof(g_state.ip_b), "0.0.0.0");

    result = tlc_openSession(&g_state.app_a, vos_dottedIP(g_state.ip_a), 0u,
                             NULL, NULL, NULL, NULL);
    if (result != TRDP_NO_ERR) {
        char message[128];
        (void)snprintf(message, sizeof(message), "tlc_openSession(Link A) failed: %d", (int)result);
        emit_error(message);
        return 0;
    }
    if (g_state.have_b) {
        result = tlc_openSession(&g_state.app_b, vos_dottedIP(g_state.ip_b), 0u,
                                 NULL, NULL, NULL, NULL);
        if (result != TRDP_NO_ERR) {
            char message[128];
            (void)tlc_closeSession(g_state.app_a);
            g_state.app_a = NULL;
            (void)snprintf(message, sizeof(message), "tlc_openSession(Link B) failed: %d", (int)result);
            emit_error(message);
            return 0;
        }
    }
    emit_ok("open", NULL);
    return 1;
}

static void handle_object_start(const char *json) {
    char id[TAU_ID_LEN], kind_text[32], link[16], transport[16];
    TAU_OBJECT *object;
    TRDP_ERR_T result = TRDP_NO_ERR;
    if (!json_string(json, "id", id, sizeof(id), NULL) ||
        !json_string(json, "kind", kind_text, sizeof(kind_text), NULL)) {
        emit_error("object_start requires object.id and object.kind");
        return;
    }
    (void)vos_mutexLock(g_state.object_mutex);
    object = find_object(id, 1);
    if (object == NULL) {
        (void)vos_mutexUnlock(g_state.object_mutex);
        emit_error("TRDP object limit reached");
        return;
    }
    object->kind = parse_kind(kind_text);
    object->com_id = json_u32(json, "com_id", 0u);
    object->cycle_us = json_u32(json, "cycle_us", 100000u);
    object->timeout_us = json_u32(json, "timeout_us", object->cycle_us > 0u ? object->cycle_us * 3u : 100000u);
    object->etb_topo = json_u32(json, "etb_topo_count", 0u);
    object->op_topo = json_u32(json, "op_trn_topo_count", 0u);
    object->red_id = json_u32(json, "red_id", 0u);
    (void)json_string(json, "destination", object->destination, sizeof(object->destination), "0.0.0.0");
    (void)json_string(json, "source", object->source, sizeof(object->source), "0.0.0.0");
    (void)json_string(json, "link", link, sizeof(link), "a");
    (void)json_string(json, "transport", transport, sizeof(transport), "udp");
    object->use_a = strcmp(link, "b") != 0;
    object->use_b = strcmp(link, "a") != 0;
    object->tcp = strcmp(transport, "tcp") == 0;
    object->payload_len = json_hex(json, "payload_hex", object->payload, sizeof(object->payload));

    if (object->kind == TAU_NONE || object->com_id == 0u) {
        (void)vos_mutexUnlock(g_state.object_mutex);
        emit_error("invalid TRDP object kind or ComID");
        return;
    }
    if (object->use_a && g_state.app_a != NULL) result = start_on_session(object, g_state.app_a, 0);
    if (result == TRDP_NO_ERR && object->use_b && g_state.app_b != NULL) result = start_on_session(object, g_state.app_b, 1);
    (void)vos_mutexUnlock(g_state.object_mutex);
    if (result != TRDP_NO_ERR) {
        char message[160];
        (void)snprintf(message, sizeof(message), "TCNOpen object_start failed: %d", (int)result);
        emit_error(message);
    } else {
        emit_ok("object_start", id);
    }
}

static void handle_object_stop(const char *json) {
    char id[TAU_ID_LEN];
    TAU_OBJECT *object;
    if (!json_string(json, "id", id, sizeof(id), NULL)) {
        emit_error("object_stop requires id");
        return;
    }
    (void)vos_mutexLock(g_state.object_mutex);
    object = find_object(id, 0);
    if (object != NULL) {
        if (g_state.app_a != NULL) stop_on_session(object, g_state.app_a, 0);
        if (g_state.app_b != NULL) stop_on_session(object, g_state.app_b, 1);
        memset(object, 0, sizeof(*object));
    }
    (void)vos_mutexUnlock(g_state.object_mutex);
    emit_ok("object_stop", id);
}

static void handle_object_update(const char *json) {
    char id[TAU_ID_LEN];
    TAU_OBJECT *object;
    if (!json_string(json, "id", id, sizeof(id), NULL)) {
        emit_error("object_update requires id");
        return;
    }
    (void)vos_mutexLock(g_state.object_mutex);
    object = find_object(id, 0);
    if (object == NULL) {
        (void)vos_mutexUnlock(g_state.object_mutex);
        emit_error("TRDP object not found");
        return;
    }
    object->payload_len = json_hex(json, "payload_hex", object->payload, sizeof(object->payload));
    if (object->pub_a != NULL && g_state.app_a != NULL) (void)tlp_put(g_state.app_a, object->pub_a, object->payload, object->payload_len);
    if (object->pub_b != NULL && g_state.app_b != NULL) (void)tlp_put(g_state.app_b, object->pub_b, object->payload, object->payload_len);
    (void)vos_mutexUnlock(g_state.object_mutex);
    emit_ok("object_update", id);
}

static void shutdown_bridge(void) {
    size_t i;
    g_state.running = 0;
    if (g_state.process_thread != NULL) {
        (void)vos_threadTerminate(g_state.process_thread);
        g_state.process_thread = NULL;
    }
    (void)vos_mutexLock(g_state.object_mutex);
    for (i = 0u; i < TAU_MAX_OBJECTS; ++i) {
        TAU_OBJECT *object = &g_state.objects[i];
        if (object->id[0] == 0) continue;
        if (g_state.app_a != NULL) stop_on_session(object, g_state.app_a, 0);
        if (g_state.app_b != NULL) stop_on_session(object, g_state.app_b, 1);
    }
    (void)vos_mutexUnlock(g_state.object_mutex);
    if (g_state.app_b != NULL) { (void)tlc_closeSession(g_state.app_b); g_state.app_b = NULL; }
    if (g_state.app_a != NULL) { (void)tlc_closeSession(g_state.app_a); g_state.app_a = NULL; }
}

int main(void) {
    char *line;
    TRDP_ERR_T trdp_result;
    memset(&g_state, 0, sizeof(g_state));
    line = (char *)malloc(TAU_LINE_LEN);
    if (line == NULL) return 2;
    if (vos_threadInit() != VOS_NO_ERR ||
        vos_mutexCreate(&g_state.object_mutex) != VOS_NO_ERR ||
        vos_mutexCreate(&g_state.output_mutex) != VOS_NO_ERR) {
        free(line);
        return 3;
    }
    trdp_result = tlc_init(NULL, NULL, NULL);
    if (trdp_result != TRDP_NO_ERR) {
        fprintf(stderr, "tlc_init failed: %d\n", (int)trdp_result);
        free(line);
        return 4;
    }
    g_state.running = 1;
    if (vos_threadCreate(&g_state.process_thread, "tauterm-trdp", VOS_THREAD_POLICY_OTHER,
                         VOS_THREAD_PRIORITY_DEFAULT, 0u, 0u, process_loop, NULL) != VOS_NO_ERR) {
        fprintf(stderr, "failed to create TRDP process thread\n");
        (void)tlc_terminate();
        free(line);
        return 5;
    }

    while (fgets(line, TAU_LINE_LEN, stdin) != NULL) {
        char command[64];
        if (!json_string(line, "command", command, sizeof(command), NULL)) {
            emit_error("missing command");
            continue;
        }
        if (strcmp(command, "open") == 0) {
            (void)open_session(line);
        } else if (strcmp(command, "object_start") == 0) {
            handle_object_start(line);
        } else if (strcmp(command, "object_stop") == 0) {
            handle_object_stop(line);
        } else if (strcmp(command, "object_update") == 0) {
            handle_object_update(line);
        } else if (strcmp(command, "capture_start") == 0 || strcmp(command, "capture_stop") == 0) {
            emit_error("live capture backend is not enabled in this bridge build");
        } else if (strcmp(command, "shutdown") == 0) {
            break;
        } else {
            emit_error("unknown bridge command");
        }
    }

    shutdown_bridge();
    (void)tlc_terminate();
    vos_threadTerm();
    free(line);
    return 0;
}
