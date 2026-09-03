#include "trdp_bridge.h"

#include <stdlib.h>
#include <string.h>

int main(void) {
    char *line = (char *)malloc(BRIDGE_MAX_LINE);
    int running = 1;
    if (line == NULL) {
        return 2;
    }

    bridge_common_init();
    while (running && fgets(line, BRIDGE_MAX_LINE, stdin) != NULL) {
        char command[64] = {0};
        if (!bridge_json_string(line, "command", command, sizeof(command), NULL)) {
            bridge_emit_error("missing command");
            continue;
        }

        if (strcmp(command, "open") == 0) {
            node_open(line);
        } else if (strcmp(command, "monitor_open") == 0) {
            bridge_emit_ack("monitor_open", NULL);
        } else if (strcmp(command, "object_start") == 0) {
            node_object_start(line);
        } else if (strcmp(command, "object_update") == 0) {
            node_object_update(line);
        } else if (strcmp(command, "object_stop") == 0) {
            node_object_stop(line);
        } else if (strcmp(command, "md_confirm") == 0) {
            node_md_confirm(line);
        } else if (strcmp(command, "md_abort") == 0) {
            node_md_abort(line);
        } else if (strcmp(command, "capture_start") == 0) {
            capture_start(line);
        } else if (strcmp(command, "capture_stop") == 0) {
            capture_stop();
            bridge_emit_ack("capture_stop", NULL);
        } else if (strcmp(command, "shutdown") == 0) {
            bridge_emit_ack("shutdown", NULL);
            running = 0;
        } else {
            bridge_emit_error("unknown TRDP bridge command");
        }
    }

    capture_shutdown();
    node_shutdown();
    bridge_common_shutdown();
    free(line);
    return 0;
}
