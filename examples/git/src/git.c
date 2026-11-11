// The docs:git/repositories interface on top of libgit2. Every function
// receives its arguments owned and must free them; the generated bindings
// free the returned values after PHP has copied them.

#include <stdlib.h>
#include <string.h>
#include <git2.h>

#include "git_reader.h"

typedef git_reader_string_t string_t;

// libgit2's settings refer to this option of its HTTP transport, which this
// build leaves out because it only reads local repositories.
bool git_http__expect_continue = false;

struct exports_docs_git_repositories_repository_t {
    git_repository *repo;
};

// A NUL-terminated copy of a WIT string, which carries a length instead.
static char *cstr(string_t *s)
{
    char *c = malloc(s->len + 1);
    memcpy(c, s->ptr, s->len);
    c[s->len] = '\0';
    git_reader_string_free(s);
    return c;
}

// Sets `err` to libgit2's last error, so PHP sees why a call failed.
static bool fail(string_t *err)
{
    const git_error *e = git_error_last();
    git_reader_string_dup(err, e && e->message ? e->message : "unknown libgit2 error");
    return false;
}

static void oid_string(string_t *ret, const git_oid *oid)
{
    char hex[GIT_OID_MAX_HEXSIZE + 1];
    git_oid_tostr(hex, sizeof hex, oid);
    git_reader_string_dup(ret, hex);
}

static void signature(exports_docs_git_repositories_signature_t *ret, const git_signature *sig)
{
    git_reader_string_dup(&ret->name, sig->name);
    git_reader_string_dup(&ret->email, sig->email);
    ret->time = sig->when.time;
    ret->offset_minutes = sig->when.offset;
}

static void commit_record(exports_docs_git_repositories_commit_t *ret, git_commit *commit)
{
    oid_string(&ret->hash, git_commit_id(commit));
    oid_string(&ret->tree, git_commit_tree_id(commit));
    size_t count = git_commit_parentcount(commit);
    ret->parents.len = count;
    ret->parents.ptr = count ? malloc(count * sizeof(string_t)) : NULL;
    for (size_t i = 0; i < count; i++) {
        oid_string(&ret->parents.ptr[i], git_commit_parent_id(commit, i));
    }
    signature(&ret->author, git_commit_author(commit));
    signature(&ret->committer, git_commit_committer(commit));
    const char *message = git_commit_message_raw(commit);
    git_reader_string_dup(&ret->message, message ? message : "");
}

// Resolves `revision` and peels it to an object of `type`. Frees `revision`.
static int lookup(git_object **out, git_repository *repo, string_t *revision, git_object_t type)
{
    char *spec = cstr(revision);
    git_object *object = NULL;
    int rc = git_revparse_single(&object, repo, spec);
    free(spec);
    if (rc < 0) {
        return rc;
    }
    if (type == GIT_OBJECT_ANY) {
        *out = object;
        return 0;
    }
    rc = git_object_peel(out, object, type);
    git_object_free(object);
    return rc;
}

bool exports_docs_git_repositories_static_repository_open(string_t *path, exports_docs_git_repositories_own_repository_t *ret, string_t *err)
{
    static bool initialized = false;
    if (!initialized) {
        git_libgit2_init();
        // WASI has no users, so every file looks owned by someone else.
        git_libgit2_opts(GIT_OPT_SET_OWNER_VALIDATION, 0);
        initialized = true;
    }

    char *dir = cstr(path);
    git_repository *repo = NULL;
    int rc = git_repository_open_ext(&repo, dir, GIT_REPOSITORY_OPEN_NO_SEARCH, NULL);
    free(dir);
    if (rc < 0) {
        return fail(err);
    }
    exports_docs_git_repositories_repository_t *rep = malloc(sizeof *rep);
    rep->repo = repo;
    *ret = exports_docs_git_repositories_repository_new(rep);
    return true;
}

void exports_docs_git_repositories_repository_destructor(exports_docs_git_repositories_repository_t *rep)
{
    git_repository_free(rep->repo);
    free(rep);
}

bool exports_docs_git_repositories_method_repository_resolve(exports_docs_git_repositories_borrow_repository_t self, string_t *revision, string_t *ret, string_t *err)
{
    git_object *object;
    if (lookup(&object, self->repo, revision, GIT_OBJECT_ANY) < 0) {
        return fail(err);
    }
    oid_string(ret, git_object_id(object));
    git_object_free(object);
    return true;
}

bool exports_docs_git_repositories_method_repository_commit(exports_docs_git_repositories_borrow_repository_t self, string_t *revision, exports_docs_git_repositories_commit_t *ret, string_t *err)
{
    git_object *object;
    if (lookup(&object, self->repo, revision, GIT_OBJECT_COMMIT) < 0) {
        return fail(err);
    }
    commit_record(ret, (git_commit *) object);
    git_object_free(object);
    return true;
}

bool exports_docs_git_repositories_method_repository_log(exports_docs_git_repositories_borrow_repository_t self, string_t *revision, uint32_t limit, exports_docs_git_repositories_list_commit_t *ret, string_t *err)
{
    git_object *start;
    if (lookup(&start, self->repo, revision, GIT_OBJECT_COMMIT) < 0) {
        return fail(err);
    }
    git_revwalk *walk;
    if (git_revwalk_new(&walk, self->repo) < 0) {
        git_object_free(start);
        return fail(err);
    }
    git_revwalk_sorting(walk, GIT_SORT_TIME);
    int rc = git_revwalk_push(walk, git_object_id(start));
    git_object_free(start);
    if (rc < 0) {
        git_revwalk_free(walk);
        return fail(err);
    }

    ret->ptr = limit ? malloc(limit * sizeof *ret->ptr) : NULL;
    ret->len = 0;
    git_oid oid;
    while (ret->len < limit && (rc = git_revwalk_next(&oid, walk)) == 0) {
        git_commit *commit;
        if ((rc = git_commit_lookup(&commit, self->repo, &oid)) < 0) {
            break;
        }
        commit_record(&ret->ptr[ret->len++], commit);
        git_commit_free(commit);
    }
    git_revwalk_free(walk);
    if (rc < 0 && rc != GIT_ITEROVER) {
        exports_docs_git_repositories_list_commit_free(ret);
        return fail(err);
    }
    return true;
}

bool exports_docs_git_repositories_method_repository_tree(exports_docs_git_repositories_borrow_repository_t self, string_t *revision, exports_docs_git_repositories_list_tree_entry_t *ret, string_t *err)
{
    git_object *object;
    if (lookup(&object, self->repo, revision, GIT_OBJECT_TREE) < 0) {
        return fail(err);
    }
    git_tree *tree = (git_tree *) object;
    size_t count = git_tree_entrycount(tree);
    ret->len = count;
    ret->ptr = count ? malloc(count * sizeof *ret->ptr) : NULL;
    for (size_t i = 0; i < count; i++) {
        const git_tree_entry *entry = git_tree_entry_byindex(tree, i);
        git_reader_string_dup(&ret->ptr[i].name, git_tree_entry_name(entry));
        oid_string(&ret->ptr[i].hash, git_tree_entry_id(entry));
        ret->ptr[i].mode = git_tree_entry_filemode_raw(entry);
        git_reader_string_dup(&ret->ptr[i].kind, git_object_type2string(git_tree_entry_type(entry)));
    }
    git_object_free(object);
    return true;
}

bool exports_docs_git_repositories_method_repository_blob(exports_docs_git_repositories_borrow_repository_t self, string_t *revision, git_reader_list_u8_t *ret, string_t *err)
{
    git_object *object;
    if (lookup(&object, self->repo, revision, GIT_OBJECT_BLOB) < 0) {
        return fail(err);
    }
    git_blob *blob = (git_blob *) object;
    ret->len = (size_t) git_blob_rawsize(blob);
    ret->ptr = malloc(ret->len ? ret->len : 1);
    memcpy(ret->ptr, git_blob_rawcontent(blob), ret->len);
    git_object_free(object);
    return true;
}
