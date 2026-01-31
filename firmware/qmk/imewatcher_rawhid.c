#include QMK_KEYBOARD_H

#include "imewatcher_rawhid.h"

// Best-effort: prefer compile-time layer count when available.
static uint8_t imewatcher_max_layers(void) {
#ifdef DYNAMIC_KEYMAP_LAYER_COUNT
    return DYNAMIC_KEYMAP_LAYER_COUNT;
#else
    return 32;
#endif
}

bool imewatcher_handle_rawhid_packet(uint8_t *data, uint8_t length) {
    (void)length; // should be 32

    if (data[0] != IMEWATCHER_CMD) {
        return false;
    }

    if (data[1] != IMEWATCHER_SIG0 || data[2] != IMEWATCHER_SIG1 || data[3] != IMEWATCHER_SIG2 || data[4] != IMEWATCHER_SIG3) {
        data[7] = IMEWATCHER_STATUS_BAD_PAYLOAD;
        return true; // handled (we consumed it)
    }

    const uint8_t opcode = data[5];
    switch (opcode) {
        case IMEWATCHER_OP_SET_DEFAULT_LAYER: {
            const uint8_t layer = data[6];
            if (layer >= imewatcher_max_layers()) {
                data[7] = IMEWATCHER_STATUS_BAD_LAYER;
                return true;
            }
            default_layer_set((layer_state_t)1 << layer);
            data[7] = IMEWATCHER_STATUS_OK;
            return true;
        }
        default: {
            data[7] = IMEWATCHER_STATUS_BAD_PAYLOAD;
            return true;
        }
    }
}

// --- Integration glue ---

// QMK(VIA): implement via_command_kb
#if defined(VIA_ENABLE) && !defined(VIAL_ENABLE)
#    include "raw_hid.h"
#    include "via.h"

bool via_command_kb(uint8_t *data, uint8_t length) {
    if (!imewatcher_handle_rawhid_packet(data, length)) {
        return false;
    }
    raw_hid_send(data, 32);
    return true;
}
#endif

// Vial-QMK: implement raw_hid_receive_kb (called for unhandled commands)
#if defined(VIAL_ENABLE)
#    include "via.h"

void raw_hid_receive_kb(uint8_t *data, uint8_t length) {
    if (imewatcher_handle_rawhid_packet(data, length)) {
        return;
    }
    data[0] = id_unhandled;
}
#endif
