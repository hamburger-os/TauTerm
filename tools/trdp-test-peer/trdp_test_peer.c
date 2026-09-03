/* Minimal TCNOpen interoperability peer for TauTerm TRDP development. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include "trdp_if_light.h"
#include "vos_sock.h"
#include "vos_thread.h"

static volatile int running = 1;
static TRDP_APP_SESSION_T app;
static TRDP_PUB_T publisher;
static TRDP_SUB_T subscriber;
static TRDP_LIS_T listener;
static UINT32 counter;

static void stop_handler(int sig) { (void)sig; running = 0; }

static void pd_cb(void *ref, TRDP_APP_SESSION_T handle, const TRDP_PD_INFO_T *info, UINT8 *data, UINT32 size) {
    (void)ref; (void)handle; (void)data;
    if (!info) return;
    printf("PD %c%c comId=%u seq=%u src=%s size=%u result=%d\n",
           (char)(info->msgType >> 8), (char)(info->msgType & 0xff), info->comId, info->seqCount,
           vos_ipDotted(info->srcIpAddr), size, info->resultCode);
    fflush(stdout);
}

static void md_cb(void *ref, TRDP_APP_SESSION_T handle, const TRDP_MD_INFO_T *info, UINT8 *data, UINT32 size) {
    TRDP_SEND_PARAM_T send;
    (void)ref; (void)data;
    if (!info) return;
    printf("MD %c%c comId=%u seq=%u size=%u result=%d replies=%u\n",
           (char)(info->msgType >> 8), (char)(info->msgType & 0xff), info->comId, info->seqCount,
           size, info->resultCode, info->numReplies);
    fflush(stdout);
    if (info->msgType == TRDP_MSG_MR) {
        static const UINT8 reply[] = {0x54,0x41,0x55,0x54,0x45,0x52,0x4d};
        memset(&send, 0, sizeof(send)); send.qos = 2; send.ttl = 64; send.retries = 2;
        (void)tlm_reply(handle, &info->sessionId, info->comId, 0, &send, reply, sizeof(reply), NULL);
    }
}

static int process_once(void) {
    TRDP_FDS_T rfds; TRDP_TIME_T tv; TRDP_SOCK_T no_desc = 0; INT32 ready;
    FD_ZERO(&rfds);
    if (tlc_getInterval(app, &tv, &rfds, &no_desc) != TRDP_NO_ERR) return -1;
    if (tv.tv_sec > 0 || tv.tv_usec > 100000) { tv.tv_sec = 0; tv.tv_usec = 100000; }
    ready = vos_select(no_desc + 1, &rfds, NULL, NULL, &tv);
    if (ready < 0) ready = 0;
    return tlc_process(app, &rfds, &ready);
}

int main(int argc, char **argv) {
    const char *mode, *own, *peer; UINT32 comid; TRDP_PD_CONFIG_T pd; TRDP_MD_CONFIG_T md; TRDP_ERR_T err;
    if (argc < 5) {
        fprintf(stderr, "usage: %s <pd-publisher|pd-subscriber|md-requester|md-replier> <own-ip> <peer/multicast-ip> <comid>\n", argv[0]);
        return 2;
    }
    mode = argv[1]; own = argv[2]; peer = argv[3]; comid = (UINT32)strtoul(argv[4], NULL, 10);
    signal(SIGINT, stop_handler); signal(SIGTERM, stop_handler);
    memset(&pd, 0, sizeof(pd)); memset(&md, 0, sizeof(md));
    pd.flags = TRDP_FLAGS_CALLBACK; pd.timeout = 300000; pd.toBehavior = TRDP_TO_KEEP_LAST_VALUE; pd.port = 17224; pd.sendParam.qos = 2; pd.sendParam.ttl = 64;
    md.flags = TRDP_FLAGS_CALLBACK; md.replyTimeout = 5000000; md.confirmTimeout = 1000000; md.connectTimeout = 60000000; md.udpPort = 17225; md.tcpPort = 17225; md.sendParam.qos = 2; md.sendParam.ttl = 64; md.sendParam.retries = 2; md.maxNumSessions = 32;
    if (tlc_init(NULL, NULL, NULL) != TRDP_NO_ERR) return 3;
    if (tlc_openSession(&app, vos_dottedIP(own), 0, NULL, &pd, &md, NULL) != TRDP_NO_ERR) return 4;

    if (!strcmp(mode, "pd-publisher")) {
        UINT8 initial[4] = {0};
        err = tlp_publish(app, &publisher, NULL, pd_cb, 0, comid, 0, 0, 0, vos_dottedIP(peer), 100000, 0, TRDP_FLAGS_CALLBACK, initial, sizeof(initial));
    } else if (!strcmp(mode, "pd-subscriber")) {
        err = tlp_subscribe(app, &subscriber, NULL, pd_cb, 0, comid, 0, 0, 0, 0, vos_dottedIP(peer), TRDP_FLAGS_CALLBACK | TRDP_FLAGS_FORCE_CB, 300000, TRDP_TO_KEEP_LAST_VALUE);
    } else if (!strcmp(mode, "md-replier")) {
        err = tlm_addListener(app, &listener, NULL, md_cb, TRUE, comid, 0, 0, 0, 0, 0, TRDP_FLAGS_CALLBACK, NULL, NULL);
    } else if (!strcmp(mode, "md-requester")) {
        TRDP_SEND_PARAM_T send; TRDP_UUID_T session; static const UINT8 request[] = {1,2,3,4};
        memset(&send, 0, sizeof(send)); send.qos = 2; send.ttl = 64; send.retries = 2;
        err = tlm_request(app, NULL, md_cb, &session, comid, 0, 0, 0, vos_dottedIP(peer), TRDP_FLAGS_CALLBACK, 1, 5000000, &send, request, sizeof(request), NULL, NULL);
    } else {
        fprintf(stderr, "unknown mode: %s\n", mode); tlc_closeSession(app); tlc_terminate(); return 2;
    }
    if (err != TRDP_NO_ERR) { fprintf(stderr, "TCNOpen operation failed: %d\n", err); tlc_closeSession(app); tlc_terminate(); return 5; }

    while (running) {
        if (!strcmp(mode, "pd-publisher")) {
            UINT8 value[4]; ++counter; value[0]=(UINT8)(counter>>24); value[1]=(UINT8)(counter>>16); value[2]=(UINT8)(counter>>8); value[3]=(UINT8)counter;
            (void)tlp_put(app, publisher, value, sizeof(value));
        }
        if (process_once() < 0) break;
    }
    if (publisher) (void)tlp_unpublish(app, publisher);
    if (subscriber) (void)tlp_unsubscribe(app, subscriber);
    if (listener) (void)tlm_delListener(app, listener);
    (void)tlc_closeSession(app); (void)tlc_terminate();
    return 0;
}
