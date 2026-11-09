#ifndef MI_FFI_H
#define MI_FFI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct WorkbenchStore WorkbenchStore;
typedef struct DocumentInstanceLock DocumentInstanceLock;

int32_t mi_core_run_qa_from_env(void);

DocumentInstanceLock *mi_core_acquire_instance_lock(const char *doc_dir_utf8);
void mi_core_release_instance_lock(DocumentInstanceLock *ptr);

WorkbenchStore *mi_core_new(const char *doc_dir_utf8);
void mi_core_free(WorkbenchStore *ptr);

char *mi_core_project_json(WorkbenchStore *ptr);
char *mi_core_dispatch_json(WorkbenchStore *ptr, const char *cmd_utf8);
char *mi_core_poll_json(WorkbenchStore *ptr);

void mi_core_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif
