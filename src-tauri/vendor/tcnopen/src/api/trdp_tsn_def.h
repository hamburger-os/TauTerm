/**********************************************************************************************************************/
/**
 * @file            trdp_tsn_def.h
 *
 * @brief           Additional definitions for TSN
 *
 * @details         This header file defines proposed extensions and additions to IEC61375-2-3:2017
 *                  The definitions herein are preliminary and may change with the next major release
 *                  of the IEC 61375-2-3 standard.
 *
 * @note            Project: TCNOpen TRDP prototype stack & FDF/DbD
 *
 * @author          Bernd Loehr, NewTec GmbH, 2019-02-19
 *
 * @remarks This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 *          If a copy of the MPL was not distributed with this file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *          Copyright Alstom SA or its subsidiaries and others, 2013-2023. All rights reserved.
 */
/*
 * $Id: trdp_tsn_def.h 2422 2023-10-23 15:57:53Z ahweiss $
 * 
 *      PL 2023-07-13: Ticket #435 Cleanup VLAN and TSN for options for Linux systems
 *     AHW 2023-06-08: Ticket #435 Cleanup VLAN and TSN options at different places
 *      PL 2023-05-19: Ticket #434 Code adaption due to TSN header version 2 removal.
 *
 */

#ifndef TRDP_TSN_DEF_H
#define TRDP_TSN_DEF_H

#ifdef __cplusplus
extern "C" {
#endif


/***********************************************************************************************************************
 * DEFINITIONS
 */


/**  Default PD communication parameters   */
#define TRDP_PD_DEFAULT_TSN_PRIORITY        7u       /* #435*/                   /**< matching new proposed priority classes */

#ifdef __cplusplus
}
#endif


#endif
