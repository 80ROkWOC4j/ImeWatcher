#pragma once

#include <stdbool.h>
#include <stdint.h>

// You can override these from config.h before including this header.
#ifndef IMEWATCHER_CMD
#    define IMEWATCHER_CMD 0x21
#endif

#define IMEWATCHER_SIG0 0x49 /* 'I' */
#define IMEWATCHER_SIG1 0x4D /* 'M' */
#define IMEWATCHER_SIG2 0x45 /* 'E' */
#define IMEWATCHER_SIG3 0x57 /* 'W' */

#define IMEWATCHER_OP_SET_DEFAULT_LAYER 0x01

#define IMEWATCHER_STATUS_OK            0x00
#define IMEWATCHER_STATUS_BAD_LAYER     0x01
#define IMEWATCHER_STATUS_BAD_PAYLOAD   0x02

// data layout (QMK indexes):
// data[0] = command
// data[1..4] = signature
// data[5] = opcode
// data[6] = layer_index
// data[7] = status (response)
bool imewatcher_handle_rawhid_packet(uint8_t *data, uint8_t length);
