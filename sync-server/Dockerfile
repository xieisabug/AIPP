FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1

WORKDIR /app

COPY pyproject.toml alembic.ini ./
COPY alembic ./alembic
COPY app ./app

RUN pip install --no-cache-dir . \
    && groupadd --system aipp && useradd --system --gid aipp aipp

# SQLite 数据默认落在 /app/data，挂卷以持久化：
#   docker run -v aipp-sync-data:/app/data ...
VOLUME ["/app/data"]
RUN mkdir -p /app/data && chown -R aipp:aipp /app

USER aipp

EXPOSE 8080

# 先应用数据库迁移再启动服务
CMD ["sh", "-c", "alembic upgrade head && uvicorn app.main:app --host 0.0.0.0 --port 8080"]
