#ifndef COMPACT_JAPANESE_H
#define COMPACT_JAPANESE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Status: 0 success, 1 invalid input, 2 insufficient capacity, 3 conversion error.
 * UTF-8 inputs must be NFKC normalized, NUL-free and at most 16384 bytes.
 * Input, output and required-length allocations must not overlap.
 * On failure output is unchanged; status 2 sets the required byte length.
 * Keep the immutable dictionary file in place while any model uses it.
 * A context can be shared by concurrent calls. Do not race cj_close with calls.
 */
uint32_t cj_abi_version(void);
const void *cj_open(const char *dictionary_path);
void cj_close(const void *context);
int32_t cj_romanize(const void *context, const uint8_t *input, size_t input_length,
                    uint8_t *output, size_t capacity, size_t *required);
int32_t cj_compile_reading(const uint8_t *input, size_t input_length,
                           uint8_t *output, size_t capacity, size_t *required);
#ifdef __cplusplus
}
#endif
#endif
