-- Studio : runs, thèmes, fichiers produits et file des jobs.

create table runs (
    id uuid primary key,
    name text not null unique,
    spec jsonb not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table themes (
    name text primary key,
    spec jsonb not null,
    updated_at timestamptz not null default now()
);

-- Un fichier par sorte et par run : le dernier produit remplace le précédent.
create table artifacts (
    run_id uuid not null references runs (id) on delete cascade,
    kind text not null check (kind in ('places', 'tour', 'video', 'cover', 'publication')),
    key text not null,
    bytes bigint not null,
    meta jsonb not null default '{}',
    job_id uuid,
    created_at timestamptz not null default now(),
    primary key (run_id, kind)
);

create table jobs (
    id uuid primary key,
    run_id uuid not null references runs (id) on delete cascade,
    kind jsonb not null,
    status text not null check (status in ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    progress jsonb,
    log jsonb not null default '[]',
    error text,
    cancel_requested boolean not null default false,
    worker text,
    heartbeat_at timestamptz,
    created_at timestamptz not null,
    started_at timestamptz,
    finished_at timestamptz
);

create index jobs_queued on jobs (created_at) where status = 'queued';
create index jobs_by_run on jobs (run_id, created_at desc);
