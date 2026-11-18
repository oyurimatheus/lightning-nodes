# Executor de tarefas de desenvolvimento. As versões das ferramentas são
# gerenciadas pelo mise. Execute `mise exec -- just --list` após `mise install`.

default:
    @just --list

# Preparar as ferramentas e o ambiente de desenvolvimento
setup:
    mise trust
    mise install
    cp -n .env.example .env || true
    @echo "Configuração concluída. Use 'mise exec -- just <comando>'."

# Executar a aplicação localmente (espera PostgreSQL iniciado com `just db-up`)
dev:
    cargo run

# Compilar a aplicação (modo de depuração)
build:
    cargo build

# Compilar o binário otimizado de release
release:
    cargo build --release

# Formatar o código
fmt:
    cargo fmt

# Validar a formatação
fmt-check:
    cargo fmt --check

# Executar Clippy tratando avisos como erros
lint:
    cargo clippy --all-targets -- -D warnings

# Executar cargo check
check:
    cargo check

# Executar testes unitários (sem rede ou Podman)
test:
    cargo test --lib

# Executar testes de integração (Testcontainers requer Podman ou runtime compatível com Docker)
test-integration:
    cargo test --test '*' -- --test-threads=1

# Executar todos os testes
test-all: test test-integration

# Iniciar o PostgreSQL de desenvolvimento
db-up:
    podman-compose -p nodes-svc -f docker/docker-compose.yaml up -d postgres
    @echo "Aguardando o PostgreSQL ficar saudável..."
    until podman-compose -p nodes-svc -f docker/docker-compose.yaml exec -T postgres sh -c 'pg_isready -U "$$POSTGRES_USER" -d "$$POSTGRES_DB"' >/dev/null 2>&1; do sleep 1; done

# Parar o PostgreSQL de desenvolvimento sem excluir os dados
db-down:
    podman-compose -p nodes-svc -f docker/docker-compose.yaml stop postgres

# Aplicar migrações ao banco de desenvolvimento
db-migrate:
    mise exec -- sqlx migrate run

# Redefinir o banco de desenvolvimento (DESTRUTIVO: remove o volume persistente)
db-reset:
    @echo "AVISO: esta operação exclui o volume do banco de desenvolvimento e todos os dados."
    @read -p "Digite 'sim' para continuar: " answer && [ "$$answer" = "sim" ]
    podman-compose -p nodes-svc -f docker/docker-compose.yaml down -v
    just db-up

# Iniciar toda a stack Compose (migrações executadas ao iniciar a aplicação)
up:
    podman-compose -p nodes-svc -f docker/docker-compose.yaml up --build -d

# Parar a stack sem remover os volumes
down:
    podman-compose -p nodes-svc -f docker/docker-compose.yaml down

# Exibir os logs da aplicação
logs:
    podman-compose -p nodes-svc -f docker/docker-compose.yaml logs -f app

# Executar benchmark (requer a stack em execução: `just up`)
benchmark:
    ./scripts/benchmark.sh

# Executar localmente todas as verificações de qualidade
ci: fmt-check lint check test-all
    cargo build --release
