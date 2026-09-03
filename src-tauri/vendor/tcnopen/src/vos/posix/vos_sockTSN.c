/**********************************************************************************************************************/
/**
 * @file            posix/vos_sockTSN.c
 *
 * @brief           Socket functions
 *
 * @details         OS abstraction of IP socket functions for TSN
 *
 * @note            Project: TCNOpen TRDP prototype stack
 *
 * @author          Bernd Loehr, NewTec GmbH
 *
 * @remarks This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 *          If a copy of the MPL was not distributed with this file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *          Copyright Alstom SA or its subsidiaries and others, 2013-2023. All rights reserved.
 */
/*
* $Id: vos_sockTSN.c 2422 2023-10-23 15:57:53Z ahweiss $
*
*      PL 2023-07-13: Ticket #435 Cleanup VLAN and TSN for options for Linux systems
*      PL 2023-04-19: Ticket #430 PC Lint Analysis and Fix
*     CWE 2023-03-28: Ticket #342 Updating TSN / VLAN / RT-thread code
*      AM 2022-12-01: Ticket #399 Abstract socket type (VOS_SOCK_T, TRDP_SOCK_T) introduced
*     AHW 2021-05-06: Ticket #322 Subscriber multicast message routing in multi-home device
*      BL 2019-06-17: Ticket #191 Add provisions for TSN / Hard Real Time (open source)
*
*/

#ifndef TSN_SUPPORT
#error \
    "You are trying to add TSN support to vos_sock.c - either define TSN_SUPPORT or exclude this file!"
#else

/***********************************************************************************************************************
 * INCLUDES
 */

#include <stdio.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>
#include <errno.h>
#include <string.h>
#include <fcntl.h>
#include <sys/socket.h>
#include <sys/ioctl.h>

#ifdef INTEGRITY
#   include <sys/uio.h>
#endif

#ifdef __linux
#   include <linux/if.h>
#   include <byteswap.h>
#else
#   include <net/if.h>
#   include <net/if_types.h>
#endif

#include <netinet/ip.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#include <sys/types.h>
#include <ifaddrs.h>

#include "vos_utils.h"
#include "vos_sock.h"
#include "vos_thread.h"
#include "vos_private.h"
#include "trdp_types.h"

/***********************************************************************************************************************
 * DEFINITIONS
 */

/***********************************************************************************************************************
 *  LOCALS
 */

/* Fallback typical interface name for VLAN interface create: used scheme = prefix followed by a dot and the unpadded VLAN-ID (0..4095) */
extern const CHAR8 *cDefaultIface;

/***********************************************************************************************************************
 * LOCAL FUNCTIONS
 */


/***********************************************************************************************************************
 * GLOBAL FUNCTIONS
 */

/**********************************************************************************************************************/
/** Create a TSN socket.
 *  Return a socket descriptor for further calls. The socket options are optional and can be
 *  applied later.
 *
 *  @param[out]     pSock           pointer to socket descriptor returned
 *  @param[in]      pOptions        pointer to socket options (optional)
 *
 *  @retval         VOS_NO_ERR      no error
 *  @retval         VOS_PARAM_ERR   pSock == NULL
 *  @retval         VOS_SOCK_ERR    socket not available or option not supported
 */

EXT_DECL VOS_ERR_T vos_sockOpenTSN (
    VOS_SOCK_T              *pSock,
    const VOS_SOCK_OPT_T    *pOptions)
{
    int sock;

    if (pSock == NULL)
    {
        vos_printLogStr(VOS_LOG_ERROR, "Parameter error\n");
        return VOS_PARAM_ERR;
    }

#if defined(VOS_USE_RAW_IP_SOCKET)
    if (pOptions->raw == TRUE)
    {
        sock = socket(PF_INET, SOCK_RAW, IPPROTO_RAW);
    }
    else
    {
        sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    }
    if (sock == -1)
#else
    if ((sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)) == -1)
#endif
    {
        char buff[VOS_MAX_ERR_STR_SIZE];
        STRING_ERR(buff);
        vos_printLog(VOS_LOG_ERROR, "socket() failed (Err: %s)\n", buff);
        return VOS_SOCK_ERR;
    }
#if defined(VOS_USE_RAW_IP_SOCKET)
    if (pOptions->raw == TRUE)
    {
        int yes = 1;
        if (setsockopt(sock, IPPROTO_IP, IP_HDRINCL, &yes, sizeof(yes)) < 0)
        {
            close(sock);
            vos_printLogStr(VOS_LOG_ERROR, "socket() setsockopt failed!\n");
            return VOS_SOCK_ERR;
        }
    }
#endif

    /* Other socket options to be applied */
    if ((vos_sockSetOptions(sock, pOptions) != VOS_NO_ERR)
        || (vos_sockSetBuffer(sock) != VOS_NO_ERR))
    {
        close(sock);
        vos_printLogStr(VOS_LOG_ERROR, "socket() failed, setsockoptions or buffer failed!\n");
        return VOS_SOCK_ERR;
    }

    *pSock = (VOS_SOCK_T) sock;

    vos_printLog(VOS_LOG_DBG, "vos_sockOpenTSN: socket()=%d success\n", (int)sock);
    return VOS_NO_ERR;
}


/**********************************************************************************************************************/
/** Debug output main socket options
 *
 *  @param[in]      sock            socket
 */
EXT_DECL void vos_sockPrintOptions (
    VOS_SOCK_T sock)
{
    int     i = 0;
    INT32   optionValues[10] = {0};
    char    buff[VOS_MAX_ERR_STR_SIZE];

    /* vos_printLog(VOS_LOG_DBG, "vos_sockPrintOptions() for socket = %d:\n", sock); */
    {
        int         sockOptValue    = 0;
        socklen_t   optSize         = sizeof(sockOptValue);

#ifdef SO_REUSEPORT
        if (getsockopt(sock, SOL_SOCKET, SO_REUSEPORT, &sockOptValue,
                       &optSize) == -1)
        {
            STRING_ERR(buff);
            vos_printLog(VOS_LOG_WARNING, "getsockopt() SO_REUSEPORT failed (Err: %s)\n", buff);
        }
#else
        if (getsockopt(sock, SOL_SOCKET, SO_REUSEADDR, &sockOptValue,
                       &optSize) == -1)
        {
            STRING_ERR(buff);
            vos_printLog(VOS_LOG_WARNING, "getsockopt() SO_REUSEADDR failed (Err: %s)\n", buff);
        }
#endif
        optionValues[i++] = (INT32) sockOptValue;
    }

#ifdef SO_PRIORITY
    {
        /* if available (and the used socket is tagged) set the VLAN PCP field as well. */
        int         sockOptValue    = 0;
        socklen_t   optSize         = sizeof(sockOptValue);
        if (getsockopt(sock, SOL_SOCKET, SO_PRIORITY, &sockOptValue,
                       &optSize) == -1)
        {
            STRING_ERR(buff);
            vos_printLog(VOS_LOG_WARNING, "getsockopt() SO_PRIORITY failed (Err: %s)\n", buff);
        }
        optionValues[i++] = (INT32) sockOptValue;
    }
#else
    optionValues[i++] = 0;
#endif
    {
        int         sockOptValue    = 0;
        socklen_t   optSize         = sizeof(sockOptValue);
        if (getsockopt(sock, SOL_SOCKET, SO_TYPE, &sockOptValue,
                       &optSize) == -1)
        {
            STRING_ERR(buff);
            vos_printLog(VOS_LOG_WARNING, "getsockopt() SO_TYPE failed (Err: %s)\n", buff);
        }
        optionValues[i++] = (INT32) sockOptValue;
    }
    {
        struct sockaddr_in  sockAddr;
        memset(&sockAddr, 0, sizeof(sockAddr));

        socklen_t           optSize = sizeof(sockAddr);
        if (getsockname(sock, (struct sockaddr *) &sockAddr, &optSize) == -1)
        {
            STRING_ERR(buff);
            vos_printLog(VOS_LOG_WARNING, "getsockname() failed (Err: %s)\n", buff);
        }
        else
        {
            const char *sType[] =
            {"### unknown!", "SOCK_STREAM", "SOCK_DGRAM", "SOCK_RAW", "SOCK_RDM", "SOCK_SEQPACKET"};
            if (optionValues[2] > 5)
            {
                optionValues[2] = 0;
            }
            vos_printLog(VOS_LOG_DBG, "        Reuse %d, prio %d, type %s\n",
                         optionValues[0],
                         optionValues[1], sType[optionValues[2]]);
            vos_printLog(VOS_LOG_DBG, "        family %d, bind %s, port %u\n",
                         sockAddr.sin_family,
                         vos_ipDotted(vos_ntohl(sockAddr.sin_addr.s_addr)),
                         vos_ntohs(sockAddr.sin_port));
        }
    }
}


/**********************************************************************************************************************/
/** Send TSN over UDP data.
 *  Send data to the supplied address and port.
 *
 *  @param[in]      sock            socket descriptor
 *  @param[in]      pBuffer         pointer to data to send
 *  @param[in,out]  pSize           In: size of the data to send, Out: no of bytes sent
 *  @param[in]      srcIpAddress    source IP
 *  @param[in]      dstIpAddress    destination IP
 *  @param[in]      port            destination port
 *  @param[in]      pTxTime         absolute time when to send this packet
 *
 *  @retval         VOS_NO_ERR      no error
 *  @retval         VOS_PARAM_ERR   sock descriptor unknown, parameter error
 *  @retval         VOS_IO_ERR      data could not be sent
 *  @retval         VOS_BLOCK_ERR   Call would have blocked in blocking mode
 */
EXT_DECL VOS_ERR_T vos_sockSendTSN (
    VOS_SOCK_T      sock,
    const UINT8     *pBuffer,
    UINT32          *pSize,
    VOS_IP4_ADDR_T  srcIpAddress,
    VOS_IP4_ADDR_T  dstIpAddress,
    UINT16          port,
    VOS_TIMEVAL_T   *pTxTime)
{
    char                control[CMSG_SPACE(sizeof(uint64_t)) + CMSG_SPACE(sizeof(clockid_t)) +
                                CMSG_SPACE(sizeof(uint8_t))];
    struct sockaddr_in  destAddr;
    ssize_t             sendSize    = 0;
    size_t              size        = 0;
    clockid_t           clkid       = CLOCK_REALTIME;
    struct cmsghdr      *cmsg;
    struct msghdr       msg;
    uint8_t             drop_if_late    = 1;
    uint64_t            txTime          = 0llu;

#if defined(VOS_USE_RAW_IP_SOCKET)
    struct iovec        iov[3];
    struct ip           ip;
    struct udphdr
    {
        u_short uh_sport;
        u_short uh_dport;
        u_short uh_ulen;
        u_short uh_sum;
    } udph;

    ip.ip_v     = IPVERSION;
    ip.ip_hl    = 5; /* hlen >> 2; 20 Bytes */
    ip.ip_tos   = 7;
#ifdef __APPLE__
    ip.ip_len = 20 + 8 + (ushort) *pSize;
#else
    ip.ip_len = vos_htons(20 + 8 + (ushort) *pSize);
#endif
    ip.ip_id            = 0;
    ip.ip_off           = 0;
    ip.ip_ttl           = 64;                       /* time to live */
    ip.ip_p             = IPPROTO_UDP;              /* protocol */
    ip.ip_sum           = 0;                        /* checksum */
    ip.ip_src.s_addr    = vos_htonl(srcIpAddress);  /* source address -> kernel chooses IP */
    ip.ip_dst.s_addr    = vos_htonl(dstIpAddress);  /* dest address */

    udph.uh_sport   = 0;
    udph.uh_dport   = vos_htons(port);
    udph.uh_ulen    = vos_htons(8 + (ushort) * pSize);
    udph.uh_sum     = 0;

#else
    struct iovec iov;
#endif
    if (pTxTime != NULL)
    {
        txTime = (uint64_t) (pTxTime->tv_usec * 1000ll)  + (uint64_t) (pTxTime->tv_sec * 1000000000ll);
    }

    size    = *pSize;
    *pSize  = 0;

    /*      We send UDP packets to the address  */
    memset(&destAddr, 0, sizeof(destAddr));
    destAddr.sin_family         = AF_INET;
    destAddr.sin_addr.s_addr    = vos_htonl(dstIpAddress);
    destAddr.sin_port           = vos_htons(port);

    memset(&msg, 0, sizeof(msg));
    msg.msg_name    = &destAddr;
    msg.msg_namelen = sizeof(destAddr);

#if defined(VOS_USE_RAW_IP_SOCKET)
    iov[0].iov_base = (void *) &ip;
    iov[0].iov_len  = sizeof(ip);
    iov[1].iov_base = (void *) &udph;
    iov[1].iov_len  = sizeof(udph);
    iov[2].iov_base = (void *) pBuffer;
    iov[2].iov_len  = size;
    msg.msg_iov     = iov;
    msg.msg_iovlen  = 3;
#else
    iov.iov_base    = (void *) pBuffer;
    iov.iov_len     = size;
    msg.msg_iov     = &iov;
    msg.msg_iovlen  = 1;
#endif


    /*
     * We specify the transmission time in the CMSG.
     */
    if (txTime)
    {
        msg.msg_control     = control;
        msg.msg_controllen  = (socklen_t) sizeof(control);

        cmsg = CMSG_FIRSTHDR(&msg);
        cmsg->cmsg_level    = SOL_SOCKET;
        cmsg->cmsg_type     = SCM_TXTIME;
        cmsg->cmsg_len      = CMSG_LEN(sizeof(uint64_t));
        *((uint64_t *) CMSG_DATA(cmsg)) = txTime;

        cmsg = CMSG_NXTHDR(&msg, cmsg);
        cmsg->cmsg_level    = SOL_SOCKET;
        cmsg->cmsg_type     = SCM_CLOCKID;
        cmsg->cmsg_len      = CMSG_LEN(sizeof(clockid_t));
        *((clockid_t *) CMSG_DATA(cmsg)) = clkid;

        cmsg = CMSG_NXTHDR(&msg, cmsg);
        cmsg->cmsg_level    = SOL_SOCKET;
        cmsg->cmsg_type     = SCM_DROP_IF_LATE;
        cmsg->cmsg_len      = CMSG_LEN(sizeof(uint8_t));
        *((uint8_t *) CMSG_DATA(cmsg)) = drop_if_late;
    }
    sendSize = sendmsg(sock, &msg, 0);

    if (sendSize == -1)
    {
        char buff[VOS_MAX_ERR_STR_SIZE];
        STRING_ERR(buff);
        vos_printLog(VOS_LOG_WARNING, "sendmsg() to %s:%u failed (Err: %s)\n",
                     inet_ntoa(destAddr.sin_addr), (unsigned int)port, buff);
        return VOS_IO_ERR;
    }
    *pSize = (UINT32) sendSize;
    return VOS_NO_ERR;
}

/**********************************************************************************************************************/
/** Receive TSN (UDP) data.
 *  The caller must provide a sufficient sized buffer. If the supplied buffer is smaller than the bytes received, *pSize
 *  will reflect the number of copied bytes and the call should be repeated until *pSize is 0 (zero).
 *  If the socket was created in blocking-mode (default), then this call will block and will only return if data has
 *  been received or the socket was closed or an error occured.
 *  If called in non-blocking mode, and no data is available, VOS_NODATA_ERR will be returned.
 *  If pointers are provided, source IP, source port and destination IP will be reported on return.
 *
 *  @param[in]      sock            socket descriptor
 *  @param[out]     pBuffer         pointer to applications data buffer
 *  @param[in,out]  pSize           pointer to the received data size
 *  @param[out]     pSrcIPAddr      pointer to source IP
 *  @param[out]     pSrcIPPort      pointer to source port
 *  @param[out]     pDstIPAddr      pointer to dest IP
 *  @param[in]      peek            if true, leave data in queue
 *
 *  @retval         VOS_NO_ERR      no error
 *  @retval         VOS_PARAM_ERR   sock descriptor unknown, parameter error
 *  @retval         VOS_IO_ERR      data could not be read
 *  @retval         VOS_NODATA_ERR  no data
 *  @retval         VOS_BLOCK_ERR   Call would have blocked in blocking mode
 */

EXT_DECL VOS_ERR_T vos_sockReceiveTSN (
    VOS_SOCK_T sock,
    UINT8      *pBuffer,
    UINT32     *pSize,
    UINT32     *pSrcIPAddr,
    UINT16     *pSrcIPPort,
    UINT32     *pDstIPAddr,
    BOOL8      peek)
{
    return vos_sockReceiveUDP(sock, pBuffer, pSize,
                              pSrcIPAddr, pSrcIPPort,
                              pDstIPAddr, NULL, peek);
}

#endif

