#ifndef TAUTERM_TRDP_BRIDGE_H
#define TAUTERM_TRDP_BRIDGE_H

#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif

#include <stdint.h>
#include <stdio.h>

#include "trdp_if_light.h"
#include "vos_sock.h"

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <windows.h>
typedef HANDLE bridge_thread_t;
typedef CRITICAL_SECTION bridge_mutex_t;
#else
#include <pthread.h>
#include <sys/time.h>
typedef pthread_t bridge_thread_t;
typedef pthread_mutex_t bridge_mutex_t;
#endif

#define BRIDGE_MAX_LINE 131072
#define BRIDGE_MAX_PAYLOAD 65536
#define BRIDGE_PD_PORT 17224u
#define BRIDGE_MD_PORT 17225u

void bridge_common_init(void);
void bridge_common_shutdown(void);
void bridge_mutex_init(bridge_mutex_t *mutex);
void bridge_mutex_destroy(bridge_mutex_t *mutex);
void bridge_mutex_lock(bridge_mutex_t *mutex);
void bridge_mutex_unlock(bridge_mutex_t *mutex);
void bridge_thread_join(bridge_thread_t thread);
void bridge_sleep_ms(unsigned int milliseconds);
uint64_t bridge_now_us(void);

void bridge_output_lock(void);
void bridge_output_unlock(void);
void bridge_json_escape(FILE *file, const char *text);
void bridge_print_hex(FILE *file, const UINT8 *data, UINT32 size);
void bridge_print_ip(FILE *file, UINT32 ip);
void bridge_emit_ack(const char *command, const char *id);
void bridge_emit_error(const char *message);
void bridge_emit_trdp_error(const char *operation, TRDP_ERR_T error);

int bridge_json_string(
    const char *line,
    const char *key,
    char *output,
    size_t capacity,
    const char *fallback
);
uint32_t bridge_json_u32(const char *line, const char *key, uint32_t fallback);
int bridge_json_bool(const char *line, const char *key, int fallback);
UINT8 *bridge_hex_decode(const char *text, UINT32 *size);
void bridge_uuid_to_text(const TRDP_UUID_T uuid, char output[37]);
int bridge_uuid_parse(const char *text, TRDP_UUID_T uuid);

void node_open(const char *line);
void node_object_start(const char *line);
void node_object_update(const char *line);
void node_object_stop(const char *line);
void node_md_confirm(const char *line);
void node_md_abort(const char *line);
void node_shutdown(void);

void capture_start(const char *line);
void capture_stop(void);
void capture_shutdown(void);

#endif
