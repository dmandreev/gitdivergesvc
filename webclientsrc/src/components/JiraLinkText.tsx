import { CONFIG } from '../config'

const JIRA_TICKET_RE = /[A-Z]{1,15}-\d{1,15}/g

interface JiraLinkTextProps {
  text: string
  className?: string
}

export function JiraLinkText({ text, className }: JiraLinkTextProps) {
  const baseUrl = CONFIG.JIRA_SERVER_ADDR
  if (!baseUrl) {
    return <span className={className}>{text}</span>
  }

  const parts: Array<{ type: 'text' | 'link'; value: string }> = []
  let lastIndex = 0

  for (const match of text.matchAll(JIRA_TICKET_RE)) {
    const start = match.index!
    const end = start + match[0].length

    if (start > lastIndex) {
      parts.push({ type: 'text', value: text.slice(lastIndex, start) })
    }

    parts.push({ type: 'link', value: match[0] })
    lastIndex = end
  }

  if (lastIndex < text.length) {
    parts.push({ type: 'text', value: text.slice(lastIndex) })
  }

  return (
    <span className={className}>
      {parts.map((part, i) =>
        part.type === 'link' ? (
          <a
            key={i}
            href={`${baseUrl}/${part.value}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-accent-light hover:underline"
            onClick={(e) => e.stopPropagation()}
          >
            {part.value}
          </a>
        ) : (
          <span key={i}>{part.value}</span>
        )
      )}
    </span>
  )
}
