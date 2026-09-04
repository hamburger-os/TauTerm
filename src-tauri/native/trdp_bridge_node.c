#include "trdp_bridge.h"

#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <process.h>
#endif

#define NODE_MAX_OBJECTS 256

typedef enum {
    NODE_OBJECT_NONE = 0,
    NODE_OBJECT_PD_PUBLISHER,
    NODE_OBJECT_PD_SUBSCRIBER,
    NODE_OBJECT_PD_REQUEST,
    NODE_OBJECT_MD_LISTENER
} node_object_kind_t;

typedef struct {
    char id[64];
    char name[96];
    char link[8];
    node_object_kind_t kind;
    UINT32 com_id;
    UINT32 etb_topo_count;
    UINT32 op_trn_topo_count;
    UINT32 red_id;
    BOOL8 red_leader;
    UINT32 reply_com_id;
    TRDP_IP_ADDR_T reply_ip;
    TRDP_TO_BEHAVIOR_T timeout_behavior;
    UINT8 *data;
    UINT32 data_len;
    TRDP_PUB_T publisher[2];
    TRDP_SUB_T subscriber[2];
    TRDP_LIS_T listener[2];
    TRDP_URI_USER_T source_uri;
    TRDP_URI_USER_T dest_uri;
    UINT32 confirm_timeout_us;
    int reply_query;
    int active;
} node_object_t;

typedef struct {
    TRDP_APP_SESSION_T app;
    UINT32 own_ip;
    char label;
    int active;
} node_link_t;

static node_link_t g_links[2];
static node_object_t g_objects[NODE_MAX_OBJECTS];
static bridge_mutex_t g_node_mutex;
static int g_node_mutex_ready;
static volatile int g_node_running;
static int g_tlc_initialized;
static bridge_thread_t g_node_thread;
static int g_node_thread_active;

static TRDP_COM_PARAM_T md_send_params(void) {
    TRDP_COM_PARAM_T send;
    memset(&send, 0, sizeof(send));
    send.qos = 2u;
    send.ttl = 64u;
    send.retries = 2u;
    return send;
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

static void emit_pd(
    node_object_t *object,
    TRDP_APP_SESSION_T app,
    const TRDP_PD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    if (message == NULL) {
        return;
    }
    bridge_output_lock();
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"pd\",\"link\":\"%c\",\"timestamp_us\":%llu,\"id\":\"",
        link_name_for_app(app),
        (unsigned long long)bridge_now_us()
    );
    bridge_json_escape(stdout, object != NULL ? object->id : "");
    fprintf(
        stdout,
        "\",\"msg_type\":\"%s\",\"com_id\":%u,\"seq_count\":%u,"
        "\"protocol_version\":%u,\"etb_topo_count\":%u,\"op_trn_topo_count\":%u,"
        "\"reply_com_id\":%u,\"service_id\":%u,\"src_ip\":\"",
        message_name(message->msgType),
        (unsigned int)message->comId,
        (unsigned int)message->seqCount,
        (unsigned int)message->protVersion,
        (unsigned int)message->etbTopoCnt,
        (unsigned int)message->opTrnTopoCnt,
        (unsigned int)message->replyComId,
        (unsigned int)message->serviceId
    );
    bridge_print_ip(stdout, message->srcIpAddr);
    fputs("\",\"dest_ip\":\"", stdout);
    bridge_print_ip(stdout, message->destIpAddr);
    fputs("\",\"reply_ip\":\"", stdout);
    bridge_print_ip(stdout, message->replyIpAddr);
    fprintf(
        stdout,
        "\",\"data_len\":%u,\"result_code\":%d,\"payload_hex\":\"",
        (unsigned int)(data != NULL ? size : 0u),
        (int)message->resultCode
    );
    bridge_print_hex(stdout, data, data != NULL ? size : 0u);
    fputs("\"}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

static void pd_callback(
    void *ref,
    TRDP_APP_SESSION_T app,
    const TRDP_PD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    node_object_t *object = message != NULL && message->pUserRef != NULL
        ? (node_object_t *)message->pUserRef
        : (node_object_t *)ref;
    emit_pd(object, app, message, data, size);
}

static void emit_md(
    node_object_t *object,
    TRDP_APP_SESSION_T app,
    const TRDP_MD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    char session_id[37];
    if (message == NULL) {
        return;
    }
    bridge_uuid_to_text(message->sessionId, session_id);
    bridge_output_lock();
    fprintf(
        stdout,
        "{\"event\":\"packet\",\"kind\":\"md\",\"link\":\"%c\",\"timestamp_us\":%llu,\"id\":\"",
        link_name_for_app(app),
        (unsigned long long)bridge_now_us()
    );
    bridge_json_escape(stdout, object != NULL ? object->id : "");
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
    bridge_print_ip(stdout, message->srcIpAddr);
    fputs("\",\"dest_ip\":\"", stdout);
    bridge_print_ip(stdout, message->destIpAddr);
    fputs("\",\"md_session_id\":\"", stdout);
    bridge_json_escape(stdout, session_id);
    fputs("\",\"src_uri\":\"", stdout);
    bridge_json_escape(stdout, (const char *)message->srcUserURI);
    fputs("\",\"dest_uri\":\"", stdout);
    bridge_json_escape(stdout, (const char *)message->destUserURI);
    fprintf(
        stdout,
        "\",\"data_len\":%u,\"result_code\":%d,\"reply_status\":%d,"
        "\"user_status\":%u,\"num_replies\":%u,\"num_expected_replies\":%u,"
        "\"num_reply_queries\":%u,\"num_confirm_sent\":%u,\"num_confirm_timeout\":%u,"
        "\"reply_timeout_us\":%u,\"about_to_die\":%s,\"payload_hex\":\"",
        (unsigned int)(data != NULL ? size : 0u),
        (int)message->resultCode,
        (int)message->replyStatus,
        (unsigned int)message->userStatus,
        (unsigned int)message->numReplies,
        (unsigned int)message->numExpReplies,
        (unsigned int)message->numRepliesQuery,
        (unsigned int)message->numConfirmSent,
        (unsigned int)message->numConfirmTimeout,
        (unsigned int)message->replyTimeout,
        message->aboutToDie ? "true" : "false"
    );
    bridge_print_hex(stdout, data, data != NULL ? size : 0u);
    fputs("\"}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

static void md_callback(
    void *ref,
    TRDP_APP_SESSION_T app,
    const TRDP_MD_INFO_T *message,
    UINT8 *data,
    UINT32 size
) {
    node_object_t *object;
    TRDP_ERR_T error;
    TRDP_COM_PARAM_T send;

    if (message == NULL) {
        return;
    }
    object = message->pUserRef != NULL
        ? (node_object_t *)message->pUserRef
        : (node_object_t *)ref;
    emit_md(object, app, message, data, size);
    if (
        object == NULL
        || !object->active
        || object->kind != NODE_OBJECT_MD_LISTENER
        || message->msgType != TRDP_MSG_MR
        || message->resultCode != TRDP_NO_ERR
    ) {
        return;
    }

    send = md_send_params();
    if (object->reply_query) {
        error = tlm_replyQuery(
            app,
            &message->sessionId,
            message->comId,
            0u,
            object->confirm_timeout_us != 0u ? object->confirm_timeout_us : 1000000u,
            &send,
            object->data,
            object->data_len,
            object->source_uri[0] != '\0' ? object->source_uri : NULL
        );
    } else {
        error = tlm_reply(
            app,
            &message->sessionId,
            message->comId,
            0u,
            &send,
            object->data,
            object->data_len,
            object->source_uri[0] != '\0' ? object->source_uri : NULL
        );
    }
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error(object->reply_query ? "tlm_replyQuery" : "tlm_reply", error);
    }
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

static int parse_ipv4_text(const char *text, TRDP_IP_ADDR_T *output) {
    const char *value = text != NULL && *text != '\0' ? text : "0.0.0.0";
    if (strcmp(value, "0.0.0.0") == 0) {
        *output = 0u;
        return 1;
    }
    *output = vos_dottedIP(value);
    return *output != 0u;
}

static node_object_t *find_object(const char *id) {
    int index;
    for (index = 0; index < NODE_MAX_OBJECTS; ++index) {
        if (g_objects[index].active && strcmp(g_objects[index].id, id) == 0) {
            return &g_objects[index];
        }
    }
    return NULL;
}

static node_object_t *allocate_object(const char *id) {
    int index;
    for (index = 0; index < NODE_MAX_OBJECTS; ++index) {
        if (!g_objects[index].active) {
            memset(&g_objects[index], 0, sizeof(g_objects[index]));
            (void)snprintf(g_objects[index].id, sizeof(g_objects[index].id), "%s", id);
            g_objects[index].active = 1;
            g_objects[index].red_leader = TRUE;
            g_objects[index].timeout_behavior = TRDP_TO_KEEP_LAST_VALUE;
            g_objects[index].confirm_timeout_us = 1000000u;
            return &g_objects[index];
        }
    }
    return NULL;
}

static void stop_object_locked(node_object_t *object) {
    int index;
    if (object == NULL || !object->active) {
        return;
    }
    for (index = 0; index < 2; ++index) {
        if (!g_links[index].active) {
            continue;
        }
        if (object->publisher[index] != NULL) {
            (void)tlp_unpublish(g_links[index].app, object->publisher[index]);
            object->publisher[index] = NULL;
        }
        if (object->subscriber[index] != NULL) {
            (void)tlp_unsubscribe(g_links[index].app, object->subscriber[index]);
            object->subscriber[index] = NULL;
        }
        if (object->listener[index] != NULL) {
            (void)tlm_delListener(g_links[index].app, object->listener[index]);
            object->listener[index] = NULL;
        }
    }
    free(object->data);
    memset(object, 0, sizeof(*object));
}

static TRDP_ERR_T open_link(
    node_link_t *link,
    char label,
    const char *ip,
    UINT16 pd_port,
    UINT16 md_udp_port,
    UINT16 md_tcp_port
) {
    TRDP_PD_CONFIG_T pd;
    TRDP_MD_CONFIG_T md;
    TRDP_PROCESS_CONFIG_T process;
    TRDP_ERR_T error;

    memset(&pd, 0, sizeof(pd));
    memset(&md, 0, sizeof(md));
    memset(&process, 0, sizeof(process));
    pd.sendParam.qos = 2u;
    pd.sendParam.ttl = 64u;
    pd.flags = TRDP_FLAGS_CALLBACK;
    pd.timeout = TRDP_DEFAULT_PD_TIMEOUT;
    pd.toBehavior = TRDP_TO_SET_TO_ZERO;
    pd.port = pd_port;

    md.sendParam = md_send_params();
    md.flags = TRDP_FLAGS_CALLBACK;
    md.replyTimeout = 5000000u;
    md.confirmTimeout = 1000000u;
    md.connectTimeout = 60000000u;
    md.sendingTimeout = 5000000u;
    md.udpPort = md_udp_port;
    md.tcpPort = md_tcp_port;
    md.maxNumSessions = 64u;

    (void)snprintf((char *)process.hostName, sizeof(process.hostName), "TauTerm");
    process.cycleTime = TRDP_PROCESS_DEFAULT_CYCLE_TIME;
    process.priority = 0u;
    process.options = TRDP_OPTION_NONE;
    process.vlanId = 0u;

    link->label = label;
    if (!parse_ipv4_text(ip, &link->own_ip)) {
        return TRDP_PARAM_ERR;
    }
    error = tlc_openSession(&link->app, link->own_ip, 0u, NULL, &pd, &md, &process);
    if (error == TRDP_NO_ERR) {
        link->active = 1;
    }
    return error;
}

#ifdef _WIN32
static unsigned __stdcall node_process_loop(void *unused)
#else
static void *node_process_loop(void *unused)
#endif
{
    (void)unused;
    for (;;) {
        int index;
        int running;
        bridge_mutex_lock(&g_node_mutex);
        running = g_node_running;
        bridge_mutex_unlock(&g_node_mutex);
        if (!running) {
            break;
        }
        for (index = 0; index < 2; ++index) {
            if (g_links[index].active) {
                TRDP_FDS_T read_fds;
                TRDP_TIME_T interval;
                TRDP_SOCK_T no_desc = 0;
                INT32 ready;
                bridge_mutex_lock(&g_node_mutex);
                FD_ZERO(&read_fds);
                if (tlc_getInterval(g_links[index].app, &interval, &read_fds, &no_desc) == TRDP_NO_ERR) {
                    if (interval.tv_sec > 0 || interval.tv_usec > 10000) {
                        interval.tv_sec = 0;
                        interval.tv_usec = 10000;
                    }
                    ready = vos_select(no_desc, &read_fds, NULL, NULL, &interval);
                    if (ready < 0) {
                        ready = 0;
                    }
                    (void)tlc_process(g_links[index].app, &read_fds, &ready);
                }
                bridge_mutex_unlock(&g_node_mutex);
            }
        }
        bridge_sleep_ms(1u);
    }
#ifdef _WIN32
    return 0u;
#else
    return NULL;
#endif
}

static int start_node_thread(void) {
    if (g_node_thread_active) {
        return 1;
    }
#ifdef _WIN32
    {
        uintptr_t thread = _beginthreadex(NULL, 0, node_process_loop, NULL, 0, NULL);
        if (thread == 0u) {
            return 0;
        }
        g_node_thread = (HANDLE)thread;
    }
#else
    if (pthread_create(&g_node_thread, NULL, node_process_loop, NULL) != 0) {
        return 0;
    }
#endif
    g_node_thread_active = 1;
    return 1;
}

static TRDP_ERR_T start_pd_publisher(
    node_object_t *object,
    int link_index,
    TRDP_IP_ADDR_T source_ip,
    TRDP_IP_ADDR_T dest_ip,
    UINT32 cycle_us
) {
    node_link_t *link = &g_links[link_index];
    TRDP_ERR_T error;
    object->kind = NODE_OBJECT_PD_PUBLISHER;
    if (object->red_id != 0u) {
        error = tlp_setRedundant(link->app, object->red_id, object->red_leader);
        if (error != TRDP_NO_ERR) {
            return error;
        }
    }
    return tlp_publish(
        link->app,
        &object->publisher[link_index],
        object,
        pd_callback,
        object->com_id,
        object->etb_topo_count,
        object->op_trn_topo_count,
        source_ip,
        dest_ip,
        cycle_us != 0u ? cycle_us : 100000u,
        object->red_id,
        TRDP_FLAGS_CALLBACK,
        object->data,
        object->data_len
    );
}

static TRDP_ERR_T start_pd_subscriber(
    node_object_t *object,
    int link_index,
    TRDP_IP_ADDR_T source_ip,
    TRDP_IP_ADDR_T dest_ip,
    UINT32 timeout_us,
    int request
) {
    node_link_t *link = &g_links[link_index];
    UINT32 subscribe_com_id = request && object->reply_com_id != 0u
        ? object->reply_com_id
        : object->com_id;
    TRDP_IP_ADDR_T source_filter = request && dest_ip != 0u ? dest_ip : source_ip;
    TRDP_IP_ADDR_T subscription_dest = request
        ? (object->reply_ip != 0u ? object->reply_ip : link->own_ip)
        : dest_ip;
    TRDP_ERR_T error;

    object->kind = request ? NODE_OBJECT_PD_REQUEST : NODE_OBJECT_PD_SUBSCRIBER;
    error = tlp_subscribe(
        link->app,
        &object->subscriber[link_index],
        object,
        pd_callback,
        subscribe_com_id,
        object->etb_topo_count,
        object->op_trn_topo_count,
        source_filter,
        0u,
        subscription_dest,
        TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB,
        timeout_us != 0u ? timeout_us : TRDP_DEFAULT_PD_TIMEOUT,
        object->timeout_behavior
    );
    if (error != TRDP_NO_ERR || !request) {
        return error;
    }
    error = tlp_request(
        link->app,
        object->subscriber[link_index],
        object->com_id,
        object->etb_topo_count,
        object->op_trn_topo_count,
        source_ip != 0u ? source_ip : link->own_ip,
        dest_ip,
        object->red_id,
        TRDP_FLAGS_CALLBACK,
        object->data,
        object->data_len,
        subscribe_com_id,
        object->reply_ip != 0u ? object->reply_ip : link->own_ip
    );
    if (error != TRDP_NO_ERR) {
        (void)tlp_unsubscribe(link->app, object->subscriber[link_index]);
        object->subscriber[link_index] = NULL;
    }
    return error;
}

static TRDP_ERR_T start_md_listener(
    node_object_t *object,
    int link_index,
    TRDP_IP_ADDR_T source_ip,
    TRDP_IP_ADDR_T dest_ip,
    int tcp
) {
    TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK;
    object->kind = NODE_OBJECT_MD_LISTENER;
    if (tcp) {
        flags |= TRDP_FLAGS_TCP;
    }
    return tlm_addListener(
        g_links[link_index].app,
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
        object->source_uri[0] != '\0' ? object->source_uri : NULL,
        object->dest_uri[0] != '\0' ? object->dest_uri : NULL
    );
}

static void emit_md_session(const char *id, char link, UINT32 com_id, const TRDP_UUID_T uuid) {
    char session_id[37];
    bridge_uuid_to_text(uuid, session_id);
    bridge_output_lock();
    fputs("{\"event\":\"md_session\",\"id\":\"", stdout);
    bridge_json_escape(stdout, id);
    fprintf(
        stdout,
        "\",\"link\":\"%c\",\"com_id\":%u,\"timestamp_us\":%llu,\"md_session_id\":\"",
        link,
        (unsigned int)com_id,
        (unsigned long long)bridge_now_us()
    );
    bridge_json_escape(stdout, session_id);
    fputs("\"}\n", stdout);
    fflush(stdout);
    bridge_output_unlock();
}

static void send_md_one_shot(
    const char *kind,
    const char *id,
    const char *link_selection,
    const char *destination,
    const char *source,
    const char *transport,
    const char *source_uri,
    const char *dest_uri,
    UINT32 com_id,
    UINT32 etb_topo_count,
    UINT32 op_trn_topo_count,
    UINT32 num_replies,
    UINT32 reply_timeout_us,
    const UINT8 *data,
    UINT32 data_len
) {
    int index;
    int sent = 0;

    for (index = 0; index < 2; ++index) {
        node_link_t *link = &g_links[index];
        TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK;
        TRDP_COM_PARAM_T send = md_send_params();
        TRDP_IP_ADDR_T source_ip;
        TRDP_IP_ADDR_T dest_ip;
        TRDP_ERR_T error;

        if (!link->active || !link_selected(link_selection, index)) {
            continue;
        }
        if (transport != NULL && strcmp(transport, "tcp") == 0) {
            flags |= TRDP_FLAGS_TCP;
        }
        source_ip = vos_dottedIP(source != NULL && *source != '\0' ? source : "0.0.0.0");
        if (source_ip == 0u) {
            source_ip = link->own_ip;
        }
        dest_ip = vos_dottedIP(destination != NULL && *destination != '\0' ? destination : "0.0.0.0");

        bridge_mutex_lock(&g_node_mutex);
        if (strcmp(kind, "md_request") == 0) {
            TRDP_UUID_T session_id;
            error = tlm_request(
                link->app,
                NULL,
                md_callback,
                &session_id,
                com_id,
                etb_topo_count,
                op_trn_topo_count,
                source_ip,
                dest_ip,
                flags,
                num_replies,
                reply_timeout_us != 0u ? reply_timeout_us : 5000000u,
                &send,
                data,
                data_len,
                source_uri != NULL && *source_uri != '\0' ? source_uri : NULL,
                dest_uri != NULL && *dest_uri != '\0' ? dest_uri : NULL
            );
            if (error == TRDP_NO_ERR) {
                emit_md_session(id, link->label, com_id, session_id);
            }
        } else {
            error = tlm_notify(
                link->app,
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
                source_uri != NULL && *source_uri != '\0' ? source_uri : NULL,
                dest_uri != NULL && *dest_uri != '\0' ? dest_uri : NULL
            );
        }
        bridge_mutex_unlock(&g_node_mutex);
        if (error != TRDP_NO_ERR) {
            bridge_emit_trdp_error(strcmp(kind, "md_request") == 0 ? "tlm_request" : "tlm_notify", error);
            return;
        }
        ++sent;
    }
    if (sent == 0) {
        bridge_emit_error("selected TRDP link is not active");
        return;
    }
    bridge_emit_ack(kind, id);
}

void node_open(const char *line) {
    char link_a_ip[64] = "0.0.0.0";
    char link_b_ip[64] = "0.0.0.0";
    int link_b_enabled = bridge_json_bool(line, "link_b_enabled", 0);
    UINT32 pd_port_value = bridge_json_u32(line, "pd_port", BRIDGE_PD_PORT);
    UINT32 md_udp_port_value = bridge_json_u32(line, "md_udp_port", BRIDGE_MD_PORT);
    UINT32 md_tcp_port_value = bridge_json_u32(line, "md_tcp_port", BRIDGE_MD_PORT);
    UINT16 pd_port;
    UINT16 md_udp_port;
    UINT16 md_tcp_port;
    TRDP_ERR_T error;

    if (!g_node_mutex_ready) {
        bridge_mutex_init(&g_node_mutex);
        g_node_mutex_ready = 1;
    }
    if (g_tlc_initialized) {
        bridge_emit_error("TRDP Node is already open");
        return;
    }
    if (!bridge_json_string(line, "link_a_ip", link_a_ip, sizeof(link_a_ip), "0.0.0.0")
        || !bridge_json_string(line, "link_b_ip", link_b_ip, sizeof(link_b_ip), "0.0.0.0")) {
        bridge_emit_error("TRDP link IP is invalid or too long");
        return;
    }
    if (pd_port_value == 0u || pd_port_value > 65535u
        || md_udp_port_value == 0u || md_udp_port_value > 65535u
        || md_tcp_port_value == 0u || md_tcp_port_value > 65535u) {
        bridge_emit_error("TRDP ports must be in range 1..65535");
        return;
    }
    pd_port = (UINT16)pd_port_value;
    md_udp_port = (UINT16)md_udp_port_value;
    md_tcp_port = (UINT16)md_tcp_port_value;

    error = tlc_init(NULL, NULL, NULL);
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error("tlc_init", error);
        return;
    }
    g_tlc_initialized = 1;
    memset(g_links, 0, sizeof(g_links));
    memset(g_objects, 0, sizeof(g_objects));

    bridge_mutex_lock(&g_node_mutex);
    error = open_link(&g_links[0], 'A', link_a_ip, pd_port, md_udp_port, md_tcp_port);
    if (error == TRDP_NO_ERR && link_b_enabled) {
        error = open_link(&g_links[1], 'B', link_b_ip, pd_port, md_udp_port, md_tcp_port);
    }
    bridge_mutex_unlock(&g_node_mutex);
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error("tlc_openSession", error);
        node_shutdown();
        return;
    }

    g_node_running = 1;
    if (!start_node_thread()) {
        bridge_emit_error("TRDP process thread failed");
        node_shutdown();
        return;
    }
    bridge_emit_ack("open", NULL);
}

void node_object_start(const char *line) {
    char id[64] = {0};
    char kind[32] = {0};
    char name[96] = {0};
    char link_selection[8] = "a";
    char destination[64] = "0.0.0.0";
    char source[64] = "0.0.0.0";
    char reply_ip_text[64] = "0.0.0.0";
    char payload[BRIDGE_MAX_PAYLOAD * 2u + 1u] = {0};
    char transport[16] = "udp";
    char timeout_behavior[16] = "keep";
    char red_state[16] = "leader";
    char response_mode[16] = "reply";
    char source_uri[sizeof(TRDP_URI_USER_T)] = {0};
    char dest_uri[sizeof(TRDP_URI_USER_T)] = {0};
    UINT32 com_id;
    UINT32 cycle_us;
    UINT32 timeout_us;
    UINT32 etb_topo_count;
    UINT32 op_trn_topo_count;
    UINT32 red_id;
    UINT32 reply_com_id;
    UINT32 num_replies;
    UINT32 reply_timeout_us;
    UINT32 confirm_timeout_us;
    UINT32 data_len = 0u;
    TRDP_IP_ADDR_T source_ip_value;
    TRDP_IP_ADDR_T destination_ip_value;
    TRDP_IP_ADDR_T reply_ip_value;
    UINT8 *data;
    node_object_t *object;
    TRDP_ERR_T error = TRDP_NO_ERR;
    int started = 0;
    int index;

    if (!g_tlc_initialized) {
        bridge_emit_error("TRDP Node is not open");
        return;
    }
    if (!bridge_json_string(line, "id", id, sizeof(id), NULL)
        || !bridge_json_string(line, "kind", kind, sizeof(kind), NULL)) {
        bridge_emit_error("object_start requires id and kind");
        return;
    }
    if (!bridge_json_string(line, "name", name, sizeof(name), "")
        || !bridge_json_string(line, "link", link_selection, sizeof(link_selection), "a")
        || !bridge_json_string(line, "destination", destination, sizeof(destination), "0.0.0.0")
        || !bridge_json_string(line, "source", source, sizeof(source), "0.0.0.0")
        || !bridge_json_string(line, "reply_ip", reply_ip_text, sizeof(reply_ip_text), "0.0.0.0")
        || !bridge_json_string(line, "payload_hex", payload, sizeof(payload), "")
        || !bridge_json_string(line, "transport", transport, sizeof(transport), "udp")
        || !bridge_json_string(line, "timeout_behavior", timeout_behavior, sizeof(timeout_behavior), "keep")
        || !bridge_json_string(line, "red_state", red_state, sizeof(red_state), "leader")
        || !bridge_json_string(line, "response_mode", response_mode, sizeof(response_mode), "reply")
        || !bridge_json_string(line, "source_uri", source_uri, sizeof(source_uri), "")
        || !bridge_json_string(line, "dest_uri", dest_uri, sizeof(dest_uri), "")) {
        bridge_emit_error("object_start contains an invalid or oversized string field");
        return;
    }
    if (!(strcmp(link_selection, "a") == 0 || strcmp(link_selection, "b") == 0 || strcmp(link_selection, "both") == 0)
        || !(strcmp(transport, "udp") == 0 || strcmp(transport, "tcp") == 0)
        || !(strcmp(timeout_behavior, "keep") == 0 || strcmp(timeout_behavior, "zero") == 0)
        || !(strcmp(red_state, "leader") == 0 || strcmp(red_state, "follower") == 0)
        || !(strcmp(response_mode, "reply") == 0 || strcmp(response_mode, "query") == 0)) {
        bridge_emit_error("object_start contains an invalid enum value");
        return;
    }
    if (!parse_ipv4_text(source, &source_ip_value)
        || !parse_ipv4_text(destination, &destination_ip_value)
        || !parse_ipv4_text(reply_ip_text, &reply_ip_value)) {
        bridge_emit_error("object_start contains an invalid IPv4 address");
        return;
    }

    com_id = bridge_json_u32(line, "com_id", 0u);
    cycle_us = bridge_json_u32(line, "cycle_us", 100000u);
    timeout_us = bridge_json_u32(line, "timeout_us", cycle_us);
    etb_topo_count = bridge_json_u32(line, "etb_topo_count", 0u);
    op_trn_topo_count = bridge_json_u32(line, "op_trn_topo_count", 0u);
    red_id = bridge_json_u32(line, "red_id", 0u);
    reply_com_id = bridge_json_u32(line, "reply_com_id", com_id);
    num_replies = bridge_json_u32(line, "num_replies", 1u);
    reply_timeout_us = bridge_json_u32(line, "reply_timeout_us", 5000000u);
    confirm_timeout_us = bridge_json_u32(line, "confirm_timeout_us", 1000000u);
    if (com_id == 0u) {
        bridge_emit_error("ComID must be non-zero");
        return;
    }

    data = bridge_hex_decode(payload, &data_len);
    if (*payload != '\0' && data == NULL) {
        bridge_emit_error("payload_hex is invalid or too large");
        return;
    }

    if (strcmp(kind, "md_request") == 0 || strcmp(kind, "md_notify") == 0) {
        send_md_one_shot(
            kind,
            id,
            link_selection,
            destination,
            source,
            transport,
            source_uri,
            dest_uri,
            com_id,
            etb_topo_count,
            op_trn_topo_count,
            num_replies,
            reply_timeout_us,
            data,
            data_len
        );
        free(data);
        return;
    }

    if (find_object(id) != NULL) {
        free(data);
        bridge_emit_error("object id already active");
        return;
    }
    object = allocate_object(id);
    if (object == NULL) {
        free(data);
        bridge_emit_error("too many TRDP objects");
        return;
    }
    (void)snprintf(object->name, sizeof(object->name), "%s", name);
    (void)snprintf(object->link, sizeof(object->link), "%s", link_selection);
    (void)snprintf((char *)object->source_uri, sizeof(object->source_uri), "%s", source_uri);
    (void)snprintf((char *)object->dest_uri, sizeof(object->dest_uri), "%s", dest_uri);
    object->com_id = com_id;
    object->etb_topo_count = etb_topo_count;
    object->op_trn_topo_count = op_trn_topo_count;
    object->red_id = red_id;
    object->red_leader = strcmp(red_state, "follower") == 0 ? FALSE : TRUE;
    object->reply_com_id = reply_com_id;
    object->reply_ip = reply_ip_value;
    object->timeout_behavior = strcmp(timeout_behavior, "zero") == 0
        ? TRDP_TO_SET_TO_ZERO
        : TRDP_TO_KEEP_LAST_VALUE;
    object->reply_query = strcmp(response_mode, "query") == 0;
    object->confirm_timeout_us = confirm_timeout_us;
    object->data = data;
    object->data_len = data_len;

    bridge_mutex_lock(&g_node_mutex);
    for (index = 0; index < 2; ++index) {
        TRDP_IP_ADDR_T source_ip;
        TRDP_IP_ADDR_T dest_ip;
        if (!g_links[index].active || !link_selected(link_selection, index)) {
            continue;
        }
        source_ip = source_ip_value;
        dest_ip = destination_ip_value;
        if (strcmp(kind, "pd_publisher") == 0) {
            error = start_pd_publisher(object, index, source_ip, dest_ip, cycle_us);
        } else if (strcmp(kind, "pd_subscriber") == 0) {
            error = start_pd_subscriber(object, index, source_ip, dest_ip, timeout_us, 0);
        } else if (strcmp(kind, "pd_request") == 0) {
            error = start_pd_subscriber(object, index, source_ip, dest_ip, timeout_us, 1);
        } else if (strcmp(kind, "md_listener") == 0) {
            error = start_md_listener(object, index, source_ip, dest_ip, strcmp(transport, "tcp") == 0);
        } else {
            error = TRDP_PARAM_ERR;
        }
        if (error != TRDP_NO_ERR) {
            break;
        }
        error = tlc_updateSession(g_links[index].app);
        if (error != TRDP_NO_ERR) {
            break;
        }
        ++started;
    }
    bridge_mutex_unlock(&g_node_mutex);

    if (error != TRDP_NO_ERR || started == 0) {
        bridge_mutex_lock(&g_node_mutex);
        stop_object_locked(object);
        bridge_mutex_unlock(&g_node_mutex);
        if (error != TRDP_NO_ERR) {
            bridge_emit_trdp_error("object_start", error);
        } else {
            bridge_emit_error("selected TRDP link is not active");
        }
        return;
    }
    bridge_emit_ack("object_start", id);
}

void node_object_update(const char *line) {
    char id[64] = {0};
    char payload[BRIDGE_MAX_PAYLOAD * 2u + 1u] = {0};
    UINT32 data_len = 0u;
    UINT8 *data;
    node_object_t *object;
    TRDP_ERR_T error = TRDP_NO_ERR;
    int index;

    if (!bridge_json_string(line, "id", id, sizeof(id), NULL)) {
        bridge_emit_error("object_update requires id");
        return;
    }
    if (!bridge_json_string(line, "payload_hex", payload, sizeof(payload), "")) {
        bridge_emit_error("object_update payload_hex is invalid or too large");
        return;
    }
    object = find_object(id);
    if (object == NULL) {
        bridge_emit_error("TRDP object is not active");
        return;
    }
    data = bridge_hex_decode(payload, &data_len);
    if (*payload != '\0' && data == NULL) {
        bridge_emit_error("payload_hex is invalid or too large");
        return;
    }

    bridge_mutex_lock(&g_node_mutex);
    if (object->kind == NODE_OBJECT_PD_PUBLISHER) {
        for (index = 0; index < 2; ++index) {
            if (g_links[index].active && object->publisher[index] != NULL) {
                error = tlp_put(g_links[index].app, object->publisher[index], data, data_len);
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
    bridge_mutex_unlock(&g_node_mutex);
    free(data);
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error("object_update", error);
        return;
    }
    bridge_emit_ack("object_update", id);
}

void node_object_stop(const char *line) {
    char id[64] = {0};
    node_object_t *object;
    if (!bridge_json_string(line, "id", id, sizeof(id), NULL)) {
        bridge_emit_error("object_stop requires id");
        return;
    }
    object = find_object(id);
    if (object != NULL) {
        bridge_mutex_lock(&g_node_mutex);
        stop_object_locked(object);
        bridge_mutex_unlock(&g_node_mutex);
    }
    bridge_emit_ack("object_stop", id);
}

static int link_index_from_text(const char *text) {
    if (text != NULL && (text[0] == 'b' || text[0] == 'B')) {
        return 1;
    }
    return 0;
}

void node_md_confirm(const char *line) {
    char session_text[64] = {0};
    char link_text[8] = "a";
    TRDP_UUID_T session_id;
    TRDP_COM_PARAM_T send = md_send_params();
    UINT16 user_status = (UINT16)bridge_json_u32(line, "user_status", 0u);
    int index;
    TRDP_ERR_T error;

    if (!bridge_json_string(line, "md_session_id", session_text, sizeof(session_text), NULL)
        || !bridge_uuid_parse(session_text, session_id)) {
        bridge_emit_error("md_confirm requires a valid md_session_id");
        return;
    }
    (void)bridge_json_string(line, "link", link_text, sizeof(link_text), "a");
    index = link_index_from_text(link_text);
    if (!g_links[index].active) {
        bridge_emit_error("selected TRDP link is not active");
        return;
    }
    bridge_mutex_lock(&g_node_mutex);
    error = tlm_confirm(g_links[index].app, &session_id, user_status, &send);
    bridge_mutex_unlock(&g_node_mutex);
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error("tlm_confirm", error);
        return;
    }
    bridge_emit_ack("md_confirm", session_text);
}

void node_md_abort(const char *line) {
    char session_text[64] = {0};
    char link_text[8] = "a";
    TRDP_UUID_T session_id;
    int index;
    TRDP_ERR_T error;

    if (!bridge_json_string(line, "md_session_id", session_text, sizeof(session_text), NULL)
        || !bridge_uuid_parse(session_text, session_id)) {
        bridge_emit_error("md_abort requires a valid md_session_id");
        return;
    }
    (void)bridge_json_string(line, "link", link_text, sizeof(link_text), "a");
    index = link_index_from_text(link_text);
    if (!g_links[index].active) {
        bridge_emit_error("selected TRDP link is not active");
        return;
    }
    bridge_mutex_lock(&g_node_mutex);
    error = tlm_abortSession(g_links[index].app, &session_id);
    bridge_mutex_unlock(&g_node_mutex);
    if (error != TRDP_NO_ERR) {
        bridge_emit_trdp_error("tlm_abortSession", error);
        return;
    }
    bridge_emit_ack("md_abort", session_text);
}

void node_shutdown(void) {
    int index;
    if (g_node_thread_active) {
        bridge_mutex_lock(&g_node_mutex);
        g_node_running = 0;
        bridge_mutex_unlock(&g_node_mutex);
        bridge_thread_join(g_node_thread);
        g_node_thread_active = 0;
    }
    if (g_node_mutex_ready) {
        bridge_mutex_lock(&g_node_mutex);
        for (index = 0; index < NODE_MAX_OBJECTS; ++index) {
            if (g_objects[index].active) {
                stop_object_locked(&g_objects[index]);
            }
        }
        for (index = 0; index < 2; ++index) {
            if (g_links[index].active) {
                (void)tlc_closeSession(g_links[index].app);
                memset(&g_links[index], 0, sizeof(g_links[index]));
            }
        }
        bridge_mutex_unlock(&g_node_mutex);
    }
    if (g_tlc_initialized) {
        (void)tlc_terminate();
        g_tlc_initialized = 0;
    }
    if (g_node_mutex_ready) {
        bridge_mutex_destroy(&g_node_mutex);
        g_node_mutex_ready = 0;
    }
    g_node_running = 0;
}
