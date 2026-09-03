/*
 * Minimal TCNOpen 3.0.0.0 interoperability peer for TauTerm TRDP development.
 *
 * This is deliberately small and protocol-focused: it is not a simulator. It
 * exists so CI and a developer laptop can exercise TauTerm against a second,
 * independently configured TCNOpen application session.
 */
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "trdp_if_light.h"
#include "vos_sock.h"

static volatile int running = 1;
static TRDP_APP_SESSION_T app;
static TRDP_PUB_T publisher;
static TRDP_SUB_T subscriber;
static TRDP_LIS_T listener;
static UINT32 counter;
static int replier_query;
static int requester_mode;

static void stop_handler(int sig) {
    (void)sig;
    running = 0;
}

static void pd_cb(
    void *ref,
    TRDP_APP_SESSION_T handle,
    const TRDP_PD_INFO_T *info,
    UINT8 *data,
    UINT32 size
) {
    (void)ref;
    (void)handle;
    (void)data;
    if (info == NULL) {
        return;
    }
    printf(
        "PD %c%c comId=%u seq=%u src=%s size=%u result=%d\n",
        (char)(info->msgType >> 8),
        (char)(info->msgType & 0xff),
        (unsigned int)info->comId,
        (unsigned int)info->seqCount,
        vos_ipDotted(info->srcIpAddr),
        (unsigned int)size,
        (int)info->resultCode
    );
    fflush(stdout);
}

static TRDP_COM_PARAM_T md_send_params(void) {
    TRDP_COM_PARAM_T send;
    memset(&send, 0, sizeof(send));
    send.qos = 2u;
    send.ttl = 64u;
    send.retries = 2u;
    return send;
}

static void md_cb(
    void *ref,
    TRDP_APP_SESSION_T handle,
    const TRDP_MD_INFO_T *info,
    UINT8 *data,
    UINT32 size
) {
    TRDP_COM_PARAM_T send;
    TRDP_ERR_T error;
    (void)ref;
    (void)data;
    if (info == NULL) {
        return;
    }
    printf(
        "MD %c%c comId=%u seq=%u size=%u result=%d replies=%u queries=%u confirms=%u\n",
        (char)(info->msgType >> 8),
        (char)(info->msgType & 0xff),
        (unsigned int)info->comId,
        (unsigned int)info->seqCount,
        (unsigned int)size,
        (int)info->resultCode,
        (unsigned int)info->numReplies,
        (unsigned int)info->numRepliesQuery,
        (unsigned int)info->numConfirmSent
    );
    fflush(stdout);

    send = md_send_params();
    if (info->msgType == TRDP_MSG_MR) {
        static const UINT8 reply[] = {0x54, 0x41, 0x55, 0x54, 0x45, 0x52, 0x4d};
        if (replier_query) {
            error = tlm_replyQuery(
                handle,
                &info->sessionId,
                info->comId,
                0u,
                1000000u,
                &send,
                reply,
                (UINT32)sizeof(reply),
                NULL
            );
        } else {
            error = tlm_reply(
                handle,
                &info->sessionId,
                info->comId,
                0u,
                &send,
                reply,
                (UINT32)sizeof(reply),
                NULL
            );
        }
        if (error != TRDP_NO_ERR) {
            fprintf(stderr, "MD reply failed: %d\n", (int)error);
        }
    } else if (requester_mode && info->msgType == TRDP_MSG_MQ) {
        error = tlm_confirm(handle, &info->sessionId, 0u, &send);
        if (error != TRDP_NO_ERR) {
            fprintf(stderr, "MD confirm failed: %d\n", (int)error);
        }
    }
}

static int process_once(void) {
    TRDP_FDS_T read_fds;
    TRDP_TIME_T interval;
    TRDP_SOCK_T no_desc = 0;
    INT32 ready;

    FD_ZERO(&read_fds);
    if (tlc_getInterval(app, &interval, &read_fds, &no_desc) != TRDP_NO_ERR) {
        return -1;
    }
    if (interval.tv_sec > 0 || interval.tv_usec > 100000) {
        interval.tv_sec = 0;
        interval.tv_usec = 100000;
    }
    ready = vos_select(no_desc, &read_fds, NULL, NULL, &interval);
    if (ready < 0) {
        ready = 0;
    }
    return (int)tlc_process(app, &read_fds, &ready);
}

static int mode_is(const char *mode, const char *name) {
    return strcmp(mode, name) == 0;
}

static int mode_has_tcp(const char *mode) {
    size_t length = strlen(mode);
    return length >= 4u && strcmp(mode + length - 4u, "-tcp") == 0;
}

static void usage(const char *program) {
    fprintf(
        stderr,
        "usage: %s <pd-publisher|pd-subscriber|md-requester|md-requester-tcp|md-replier|md-replier-query|md-replier-tcp|md-replier-query-tcp> <own-ip> <peer/multicast-ip> <comid> [seconds]\n",
        program
    );
}

int main(int argc, char **argv) {
    const char *mode;
    const char *own;
    const char *peer;
    UINT32 comid;
    unsigned int duration_seconds = 0u;
    time_t deadline = 0;
    TRDP_PD_CONFIG_T pd;
    TRDP_MD_CONFIG_T md;
    TRDP_PROCESS_CONFIG_T process;
    TRDP_ERR_T error = TRDP_NO_ERR;

    if (argc < 5) {
        usage(argv[0]);
        return 2;
    }
    mode = argv[1];
    own = argv[2];
    peer = argv[3];
    comid = (UINT32)strtoul(argv[4], NULL, 10);
    if (argc >= 6) {
        duration_seconds = (unsigned int)strtoul(argv[5], NULL, 10);
        if (duration_seconds > 0u) {
            deadline = time(NULL) + (time_t)duration_seconds;
        }
    }
    if (comid == 0u) {
        usage(argv[0]);
        return 2;
    }

    replier_query = mode_is(mode, "md-replier-query") || mode_is(mode, "md-replier-query-tcp");
    requester_mode = mode_is(mode, "md-requester") || mode_is(mode, "md-requester-tcp");
    signal(SIGINT, stop_handler);
    signal(SIGTERM, stop_handler);

    memset(&pd, 0, sizeof(pd));
    memset(&md, 0, sizeof(md));
    memset(&process, 0, sizeof(process));
    pd.flags = TRDP_FLAGS_CALLBACK;
    pd.timeout = 300000u;
    pd.toBehavior = TRDP_TO_KEEP_LAST_VALUE;
    pd.port = 17224u;
    pd.sendParam.qos = 2u;
    pd.sendParam.ttl = 64u;
    md.flags = TRDP_FLAGS_CALLBACK;
    md.replyTimeout = 5000000u;
    md.confirmTimeout = 1000000u;
    md.connectTimeout = 60000000u;
    md.sendingTimeout = 5000000u;
    md.udpPort = 17225u;
    md.tcpPort = 17225u;
    md.sendParam = md_send_params();
    md.maxNumSessions = 32u;

    (void)snprintf((char *)process.hostName, sizeof(process.hostName), "TauTermPeer");
    process.cycleTime = TRDP_PROCESS_DEFAULT_CYCLE_TIME;
    process.priority = 0u;
    process.options = TRDP_OPTION_NONE;
    process.vlanId = 0u;

    error = tlc_init(NULL, NULL, NULL);
    if (error != TRDP_NO_ERR) {
        fprintf(stderr, "tlc_init failed: %d\n", (int)error);
        return 3;
    }
    error = tlc_openSession(&app, vos_dottedIP(own), 0u, NULL, &pd, &md, &process);
    if (error != TRDP_NO_ERR) {
        fprintf(stderr, "tlc_openSession failed: %d\n", (int)error);
        (void)tlc_terminate();
        return 4;
    }

    if (mode_is(mode, "pd-publisher")) {
        UINT8 initial[4] = {0};
        error = tlp_publish(
            app,
            &publisher,
            NULL,
            pd_cb,
            0u,
            comid,
            0u,
            0u,
            0u,
            vos_dottedIP(peer),
            100000u,
            0u,
            TRDP_FLAGS_CALLBACK,
            initial,
            (UINT32)sizeof(initial)
        );
    } else if (mode_is(mode, "pd-subscriber")) {
        error = tlp_subscribe(
            app,
            &subscriber,
            NULL,
            pd_cb,
            0u,
            comid,
            0u,
            0u,
            0u,
            0u,
            vos_dottedIP(peer),
            TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB,
            300000u,
            TRDP_TO_KEEP_LAST_VALUE
        );
    } else if (
        mode_is(mode, "md-replier")
        || mode_is(mode, "md-replier-query")
        || mode_is(mode, "md-replier-tcp")
        || mode_is(mode, "md-replier-query-tcp")
    ) {
        TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK;
        if (mode_has_tcp(mode)) {
            flags |= TRDP_FLAGS_TCP;
        }
        error = tlm_addListener(
            app,
            &listener,
            NULL,
            md_cb,
            TRUE,
            comid,
            0u,
            0u,
            0u,
            0u,
            0u,
            flags,
            NULL,
            NULL
        );
    } else if (requester_mode) {
        TRDP_COM_PARAM_T send = md_send_params();
        TRDP_UUID_T session;
        TRDP_FLAGS_T flags = TRDP_FLAGS_CALLBACK;
        static const UINT8 request[] = {1, 2, 3, 4};
        if (mode_has_tcp(mode)) {
            flags |= TRDP_FLAGS_TCP;
        }
        error = tlm_request(
            app,
            NULL,
            md_cb,
            &session,
            comid,
            0u,
            0u,
            vos_dottedIP(own),
            vos_dottedIP(peer),
            flags,
            1u,
            5000000u,
            &send,
            request,
            (UINT32)sizeof(request),
            NULL,
            NULL
        );
    } else {
        usage(argv[0]);
        (void)tlc_closeSession(app);
        (void)tlc_terminate();
        return 2;
    }

    if (error != TRDP_NO_ERR) {
        fprintf(stderr, "TCNOpen operation failed: %d\n", (int)error);
        (void)tlc_closeSession(app);
        (void)tlc_terminate();
        return 5;
    }
    error = tlc_updateSession(app);
    if (error != TRDP_NO_ERR) {
        fprintf(stderr, "tlc_updateSession failed: %d\n", (int)error);
        (void)tlc_closeSession(app);
        (void)tlc_terminate();
        return 6;
    }

    while (running) {
        if (deadline != 0 && time(NULL) >= deadline) {
            break;
        }
        if (mode_is(mode, "pd-publisher")) {
            UINT8 value[4];
            ++counter;
            value[0] = (UINT8)(counter >> 24);
            value[1] = (UINT8)(counter >> 16);
            value[2] = (UINT8)(counter >> 8);
            value[3] = (UINT8)counter;
            (void)tlp_put(app, publisher, value, (UINT32)sizeof(value));
        }
        if (process_once() < 0) {
            break;
        }
    }

    if (publisher != NULL) {
        (void)tlp_unpublish(app, publisher);
    }
    if (subscriber != NULL) {
        (void)tlp_unsubscribe(app, subscriber);
    }
    if (listener != NULL) {
        (void)tlm_delListener(app, listener);
    }
    (void)tlc_closeSession(app);
    (void)tlc_terminate();
    return 0;
}
